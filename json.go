package rulekit

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net"
	"strconv"
	"strings"
)

type JSONOptions struct {
	// AnnotatedKeys enables suffix-based type hints such as "src.$ip".
	AnnotatedKeys bool
}

// DecodeJSON decodes a JSON object into a Rulekit KV value map.
func DecodeJSON(data []byte, opts JSONOptions) (KV, error) {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()

	var raw any
	if err := dec.Decode(&raw); err != nil {
		return nil, err
	}
	normalized, err := normalizeJSONValue(raw, opts)
	if err != nil {
		return nil, err
	}
	kv, ok := normalized.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("json root must be an object")
	}
	return KV(kv), nil
}

func normalizeJSONValue(value any, opts JSONOptions) (any, error) {
	switch v := value.(type) {
	case map[string]any:
		if typ, ok := v["$type"].(string); ok {
			return decodeTypedJSONValue(typ, v)
		}
		out := make(map[string]any, len(v))
		for key, child := range v {
			outKey := key
			var suffix string
			if opts.AnnotatedKeys {
				outKey, suffix, _ = splitAnnotatedKey(key)
			}
			if _, exists := out[outKey]; exists {
				return nil, fmt.Errorf("duplicate normalized key %q", outKey)
			}
			normalized, err := normalizeJSONValue(child, opts)
			if err != nil {
				return nil, fmt.Errorf("key %q: %w", key, err)
			}
			if suffix != "" {
				normalized, err = decodeAnnotatedJSONValue(suffix, normalized)
				if err != nil {
					return nil, fmt.Errorf("key %q: %w", key, err)
				}
			}
			out[outKey] = normalized
		}
		return out, nil
	case []any:
		out := make([]any, len(v))
		for i, child := range v {
			normalized, err := normalizeJSONValue(child, opts)
			if err != nil {
				return nil, fmt.Errorf("index %d: %w", i, err)
			}
			out[i] = normalized
		}
		return out, nil
	case json.Number:
		return normalizeJSONNumber(v)
	default:
		return value, nil
	}
}

func normalizeJSONNumber(n json.Number) (any, error) {
	raw := n.String()
	if strings.ContainsAny(raw, ".eE") {
		return strconv.ParseFloat(raw, 64)
	}
	if i, err := strconv.ParseInt(raw, 10, 64); err == nil {
		return i, nil
	}
	if u, err := strconv.ParseUint(raw, 10, 64); err == nil {
		return u, nil
	}
	return nil, fmt.Errorf("invalid json number %q", raw)
}

func splitAnnotatedKey(key string) (string, string, bool) {
	for _, suffix := range []string{".$bytes_base64", ".$bytes_hex", ".$float64", ".$uint64", ".$int64", ".$cidr", ".$mac", ".$ip"} {
		if strings.HasSuffix(key, suffix) {
			return strings.TrimSuffix(key, suffix), strings.TrimPrefix(suffix, "."), true
		}
	}
	return key, "", false
}

func decodeAnnotatedJSONValue(suffix string, value any) (any, error) {
	return decodeScalarValue(strings.TrimPrefix(suffix, "$"), value, "")
}

func decodeTypedJSONValue(typ string, object map[string]any) (any, error) {
	value, ok := object["value"]
	if !ok {
		return nil, fmt.Errorf("typed json value requires value")
	}
	encoding, _ := object["encoding"].(string)
	return decodeScalarValue(typ, value, encoding)
}

func decodeScalarValue(typ string, value any, encoding string) (any, error) {
	switch typ {
	case "ip":
		s, err := stringScalar(value)
		if err != nil {
			return nil, err
		}
		ip := net.ParseIP(s)
		if ip == nil {
			return nil, fmt.Errorf("invalid ip %q", s)
		}
		return ip, nil
	case "cidr":
		s, err := stringScalar(value)
		if err != nil {
			return nil, err
		}
		_, ipnet, err := net.ParseCIDR(s)
		return ipnet, err
	case "mac":
		s, err := stringScalar(value)
		if err != nil {
			return nil, err
		}
		return net.ParseMAC(s)
	case "bytes":
		return decodeBytes(value, encoding)
	case "bytes_hex":
		return decodeBytes(value, "hex")
	case "bytes_base64":
		return decodeBytes(value, "base64")
	case "int64":
		return int64Scalar(value)
	case "uint64":
		return uint64Scalar(value)
	case "float64":
		return float64Scalar(value)
	default:
		return nil, fmt.Errorf("unknown type %q", typ)
	}
}

func decodeBytes(value any, encoding string) ([]byte, error) {
	s, err := stringScalar(value)
	if err != nil {
		return nil, err
	}
	switch encoding {
	case "hex":
		return hex.DecodeString(strings.ReplaceAll(s, ":", ""))
	case "base64":
		return base64.StdEncoding.DecodeString(s)
	default:
		return nil, fmt.Errorf("bytes encoding must be hex or base64")
	}
}

func stringScalar(value any) (string, error) {
	s, ok := value.(string)
	if !ok {
		return "", fmt.Errorf("expected string, got %T", value)
	}
	return s, nil
}

func int64Scalar(value any) (int64, error) {
	s, err := numericString(value)
	if err != nil {
		return 0, err
	}
	return strconv.ParseInt(s, 10, 64)
}

func uint64Scalar(value any) (uint64, error) {
	s, err := numericString(value)
	if err != nil {
		return 0, err
	}
	return strconv.ParseUint(s, 10, 64)
}

func float64Scalar(value any) (float64, error) {
	s, err := numericString(value)
	if err != nil {
		return 0, err
	}
	return strconv.ParseFloat(s, 64)
}

func numericString(value any) (string, error) {
	switch v := value.(type) {
	case json.Number:
		return v.String(), nil
	case string:
		return v, nil
	case int64:
		return strconv.FormatInt(v, 10), nil
	case uint64:
		return strconv.FormatUint(v, 10), nil
	case float64:
		return strconv.FormatFloat(v, 'f', -1, 64), nil
	default:
		return "", fmt.Errorf("expected number or string, got %T", value)
	}
}
