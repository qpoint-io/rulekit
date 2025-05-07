package rulekit

// // nodeFunction represents a function call in the rule
// type nodeFunction struct {
// 	name string
// 	args []Valuer
// }

// // Eval implements the Rule interface
// func (n *nodeFunction) Eval(ctx KV) Result {
// 	// Evaluate all arguments first
// 	argValues := make([]any, len(n.args))
// 	missingFields := set.NewSet[string]()
// 	for i, arg := range n.args {
// 		val, ok := arg.Value(ctx)
// 		if !ok {
// 			if field, isField := arg.(FieldValue); isField {
// 				missingFields.Add(string(field))
// 			}
// 			return Result{
// 				Pass:          false,
// 				MissingFields: missingFields,
// 				EvaluatedRule: n,
// 			}
// 		}
// 		argValues[i] = val
// 	}

// 	// Call the appropriate function based on name
// 	switch strings.ToLower(n.name) {
// 	case "starts_with":
// 		if len(argValues) != 2 {
// 			return Result{
// 				Pass:          false,
// 				MissingFields: missingFields,
// 				EvaluatedRule: n,
// 			}
// 		}
// 		str, ok1 := argValues[0].(string)
// 		prefix, ok2 := argValues[1].(string)
// 		if !ok1 || !ok2 {
// 			return Result{
// 				Pass:          false,
// 				MissingFields: missingFields,
// 				EvaluatedRule: n,
// 			}
// 		}
// 		return Result{
// 			Pass:          strings.HasPrefix(str, prefix),
// 			MissingFields: missingFields,
// 			EvaluatedRule: n,
// 		}

// 	case "domain":
// 		val, ok := n.Value(ctx)
// 		if !ok {
// 			return Result{
// 				Pass:          false,
// 				MissingFields: missingFields,
// 				EvaluatedRule: n,
// 			}
// 		}
// 		return (&nodeNotZero{
// 			rv: &LiteralValue[string]{
// 				raw:   n.String(),
// 				value: fmt.Sprint(val),
// 			},
// 		}).Eval(ctx)

// 	default:
// 		return Result{
// 			Pass:          false,
// 			MissingFields: missingFields,
// 			EvaluatedRule: n,
// 		}
// 	}
// }

// // Value implements the Valuer interface
// func (n *nodeFunction) Value(ctx KV) (any, bool) {
// 	// Evaluate all arguments first
// 	argValues := make([]any, len(n.args))
// 	missingFields := set.NewSet[string]()
// 	for i, arg := range n.args {
// 		val, ok := arg.Value(ctx)
// 		if !ok {
// 			if field, isField := arg.(FieldValue); isField {
// 				missingFields.Add(string(field))
// 			}
// 			return nil, false
// 		}
// 		argValues[i] = val
// 	}

// 	// Call the appropriate function based on name
// 	switch strings.ToLower(n.name) {
// 	case "domain":
// 		s, err := publicsuffix.EffectiveTLDPlusOne(fmt.Sprint(argValues[0]))
// 		if err != nil {
// 			return nil, false
// 		}
// 		return s, true
// 	}

// 	return nil, false
// }

// // String implements the fmt.Stringer interface
// func (n *nodeFunction) String() string {
// 	args := make([]string, len(n.args))
// 	for i, arg := range n.args {
// 		args[i] = arg.String()
// 	}
// 	return fmt.Sprintf("%s(%s)", n.name, strings.Join(args, ", "))
// }
