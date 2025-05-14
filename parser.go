package rulekit

import (
	"fmt"
	"io"
	"net"
	"os"
	"regexp"
	"strconv"
	"strings"
)

var ruleDebugWriter io.Writer = os.Stderr

func init() {
	SetErrorVerbose(true) // default to true
}

// SetDebugLevel sets the debug verbosity level
func SetDebugLevel(level int) {
	ruleDebug = level
}

func SetDebugWriter(w io.Writer) {
	ruleDebugWriter = w
}

// SetErrorVerbose enables or disables verbose error reporting
func SetErrorVerbose(verbose bool) {
	ruleErrorVerbose = verbose
}

func operatorToString(op int) string {
	switch op {
	case op_EQ:
		return "=="
	case op_NE:
		return "!="
	case op_GT:
		return ">"
	case op_GE:
		return ">="
	case op_LT:
		return "<"
	case op_LE:
		return "<="
	case op_CONTAINS:
		return "contains"
	case op_MATCHES:
		return "matches"
	case op_IN:
		return "in"
	default:
		return "unknown"
	}
}

func parseString[T interface{ string | []byte }](data T) (any, error) {
	str := string(data)
	if str[0] == '\'' {
		// Convert single-quoted string to double-quoted
		str = str[1 : len(str)-1]
		str = strings.ReplaceAll(str, `"`, "\\\"")
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	var err error
	str, err = strconv.Unquote(str)
	if err != nil {
		return nil, err
	}

	// Try to parse string value as a more specific type
	if ip := net.ParseIP(str); ip != nil {
		// Try IP address
		return ip, nil
	} else if _, ipnet, err := net.ParseCIDR(str); err == nil {
		// Try CIDR notation
		return ipnet, nil
	} else if strings.Count(str, ":") == 5 || strings.Count(str, ":") == 7 {
		// Try MAC address (if it looks like a MAC address format)
		if mac, err := net.ParseMAC(str); err == nil {
			return mac, nil
		}
	}
	return str, nil
}

func parseInt[T interface{ string | []byte }](data T) (any, error) {
	raw := string(data)
	if n, err := strconv.ParseInt(raw, 0, 64); err == nil {
		return n, nil
	}
	if n, err := strconv.ParseUint(raw, 0, 64); err == nil {
		return n, nil
	}
	return nil, fmt.Errorf("parsing integer: invalid value %q", raw)
}

func parseFloat[T interface{ string | []byte }](data T) (float64, error) {
	return strconv.ParseFloat(string(data), 64)
}

func parseBool[T interface{ string | []byte }](data T) (bool, error) {
	var val bool
	_, err := fmt.Sscanf(string(data), "%t", &val)
	return val, err
}

func parseRegex[T interface{ string | []byte }](data T) (*regexp.Regexp, error) {
	raw := string(data)
	pattern := raw[1 : len(raw)-1] // Remove the forward slashes
	return regexp.Compile(pattern)
}

func parseValueToken(typ int, rawBytes []byte) (Rule, error) {
	raw := string(rawBytes)
	var (
		value any
		err   error
	)
	switch typ {
	case token_STRING:
		value, err = parseString(raw)
	case token_INT:
		value, err = parseInt(raw)
	case token_FLOAT:
		value, err = parseFloat(raw)
	case token_BOOL:
		value, err = parseBool(raw)
	case token_IP:
		value = net.ParseIP(raw)
	case token_IP_CIDR:
		_, value, err = net.ParseCIDR(raw)
	case token_HEX_STRING:
		value, err = ParseHexString(raw)
	case token_REGEX:
		value, err = parseRegex(raw)
	default:
		err = fmt.Errorf("unknown parseValueToken type")
	}
	if err != nil {
		return nil, ValueParseError{typ, string(raw), err}
	}
	return &LiteralValue[any]{
		raw:   string(raw),
		value: value,
	}, nil
}

type valueToken struct {
	typ   int
	raw   string
	value any
}

func valueTokenString(typ int) string {
	switch typ {
	case token_STRING:
		return "string"
	case token_INT:
		return "integer"
	case token_FLOAT:
		return "float"
	case token_BOOL:
		return "boolean"
	case token_IP:
		return "IP"
	case token_IP_CIDR:
		return "CIDR"
	case token_HEX_STRING:
		return "hex string"
	case token_REGEX:
		return "regex"
	case token_FIELD:
		return "field identifier"
	case token_FUNCTION:
		return "function"
	case token_LPAREN:
		return `"("`
	case token_RPAREN:
		return `")"`
	case token_LBRACKET:
		return `"["`
	case token_RBRACKET:
		return `"]"`
	case token_COMMA:
		return `","`
	default:
		return "unknown"
	}
}

type ValueParseError struct {
	TokenType int
	Value     string
	Err       error
}

func (e ValueParseError) Error() string {
	return fmt.Sprintf("parsing %s value %q: %v", valueTokenString(e.TokenType), e.Value, e.Err)
}

type functionCall struct {
	fn   string
	args []Rule
}
