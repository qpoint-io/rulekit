package rulekit

import (
	"context"
	"net"
	"regexp"
	"unsafe"
)

// This file holds all of the unsafe code for [Func]: calls write each
// argument directly into the argument struct, and read a result of a named
// type as its underlying type, so they need no reflection.
//
// Invariant: an argSpec is built by newArgSpec from the argument struct
// type A of the same Func call, so each argField.offset is the offset of a
// field whose type is, or has as its underlying type, exactly the type its
// kind names in valueKinds (restOffset: exactly Rest). A type and its
// underlying type have the same layout. fill writes each field only through
// a pointer of the type its kind names. newCall is the only place an
// argSpec meets a pointer, and it pairs them by construction. Likewise
// newCall's returns is kindOf(R), so resultValue reads an R as the type of
// that kind.

// callArgs holds the evaluated arguments of a call. It is passed to
// Function.call by value: a slice of a caller-owned array would escape to
// the heap through the indirect call.
type callArgs struct {
	inline [4]any
	// extra holds all of the arguments when there are more than fit inline.
	extra []any
	n     int
}

func (c *callArgs) at(i int) any {
	if c.extra != nil {
		return c.extra[i]
	}
	return c.inline[i]
}

// newCall returns the call implementation of a Func. returns is the kind of
// R, and exact is true if R is the type that kind names.
func newCall[A, R any](name string, spec *argSpec, handler func(context.Context, A) (R, error), returns argKind, exact bool) func(context.Context, callArgs) Result {
	return func(ctx context.Context, args callArgs) Result {
		var a A
		if err := spec.fill(unsafe.Pointer(&a), &args); err != nil {
			return Result{Error: err}
		}
		r, err := handler(ctx, a)
		if err != nil {
			return handlerResult(name, err)
		}
		if exact {
			return Result{Value: r}
		}
		return Result{Value: resultValue(returns, unsafe.Pointer(&r))}
	}
}

// resultValue reads a result of a named type at p as the type its kind
// names.
func resultValue(kind argKind, p unsafe.Pointer) any {
	switch kind {
	case kindBool:
		return *(*bool)(p)
	case kindInt64:
		return *(*int64)(p)
	case kindUint64:
		return *(*uint64)(p)
	case kindFloat64:
		return *(*float64)(p)
	case kindString:
		return *(*string)(p)
	case kindBytes:
		return *(*[]byte)(p)
	case kindArray:
		return *(*[]any)(p)
	case kindObject:
		return *(*map[string]any)(p)
	}
	panic("rulekit: unreachable result kind " + kindNames[kind])
}

// fill converts the arguments into the fields of the argument struct at p.
// The caller has checked the number of arguments.
func (s *argSpec) fill(p unsafe.Pointer, args *callArgs) error {
	for i := range s.fields {
		f := &s.fields[i]
		v := args.at(i)
		dst := unsafe.Add(p, f.offset)
		ok := true
		switch f.kind {
		case kindAny:
			*(*any)(dst) = v
		case kindBool:
			*(*bool)(dst), ok = asBool(v)
		case kindInt64:
			*(*int64)(dst), ok = asInt64(v)
		case kindUint64:
			*(*uint64)(dst), ok = asUint64(v)
		case kindFloat64:
			*(*float64)(dst), ok = asFloat64(v)
		case kindString:
			*(*string)(dst), ok = asString(v)
		case kindBytes:
			*(*[]byte)(dst), ok = asBytes(v)
		case kindIP:
			*(*net.IP)(dst), ok = v.(net.IP)
		case kindCIDR:
			*(**net.IPNet)(dst), ok = v.(*net.IPNet)
		case kindMAC:
			*(*net.HardwareAddr)(dst), ok = v.(net.HardwareAddr)
		case kindURL:
			*(*URL)(dst), ok = asURL(v)
		case kindRegex:
			*(**regexp.Regexp)(dst), ok = v.(*regexp.Regexp)
		case kindArray:
			*(*[]any)(dst), ok = asArray(v)
		case kindObject:
			*(*map[string]any)(dst), ok = v.(map[string]any)
		}
		if !ok {
			return f.invalid(v)
		}
	}
	if s.rest && args.n > len(s.fields) {
		// Copied so the handler may keep it.
		rest := make(Rest, args.n-len(s.fields))
		for i := range rest {
			rest[i] = args.at(len(s.fields) + i)
		}
		*(*Rest)(unsafe.Add(p, s.restOffset)) = rest
	}
	return nil
}
