// Code generated by goyacc -v y.output -o parser.go -p rule parser.y. DO NOT EDIT.

//line parser.y:1

package rulekit

import __yyfmt__ "fmt"

//line parser.y:3

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

func withNegate(negate bool, node Rule) Rule {
	if negate {
		return &nodeNot{right: node}
	}
	return node
}

// Add these type-specific parsing functions in the Go code section
func parseString[T interface{ string | []byte }](data T) (string, error) {
	str := string(data)
	if str[0] == '\'' {
		// Convert single-quoted string to double-quoted
		str = str[1 : len(str)-1]
		str = strings.ReplaceAll(str, `"`, "\\\"")
		str = strings.ReplaceAll(str, `\'`, `'`)
		str = `"` + str + `"`
	}
	return strconv.Unquote(str)
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
	raw := string(data)
	if strings.EqualFold(raw, "true") {
		return true, nil
	}
	if strings.EqualFold(raw, "false") {
		return false, nil
	}
	return false, fmt.Errorf("parsing boolean: unknown value %q", raw)
}

func parseRegex[T interface{ string | []byte }](data T) (*regexp.Regexp, error) {
	raw := string(data)
	pattern := raw[1 : len(raw)-1] // Remove the forward slashes
	return regexp.Compile(pattern)
}

func newValueToken(token_type int, data []byte) (valueToken, error) {
	v := valueToken{typ: token_type, raw: string(data)}
	if err := v.Parse(); err != nil {
		return valueToken{}, err
	}
	return v, nil
}

type valueToken struct {
	typ   int
	raw   string
	value any
}

func (v *valueToken) Parse() error {
	var (
		value any
		err   error
	)
	switch v.typ {
	case token_STRING:
		value, err = parseString(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_INT:
		value, err = parseInt(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_FLOAT:
		value, err = parseFloat(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_BOOL:
		value, err = parseBool(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_IP:
		value = net.ParseIP(v.raw)
		if value == nil {
			err = ValueParseError{v.typ, v.raw, fmt.Errorf("invalid IP value %q", v.raw)}
		}
	case token_IP_CIDR:
		_, value, err = net.ParseCIDR(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_HEX_STRING:
		value, err = ParseHexString(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	case token_REGEX:
		value, err = parseRegex(v.raw)
		if err != nil {
			err = ValueParseError{v.typ, v.raw, err}
		}
	default:
		err = fmt.Errorf("unsupported token type %s", valueTokenString(v.typ))
	}
	if err != nil {
		return err
	}
	v.value = value
	return nil
}

func (v *valueToken) Valuer() Valuer {
	return LiteralValue[any]{
		raw:   v.raw,
		value: v.value,
	}
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
	default:
		return "unknown"
	}
}

func makeCompareNode(field string, negate bool, op int, elem valueToken) Rule {
	return withNegate(negate, &nodeCompare{
		lv: FieldValue(field),
		op: op,
		rv: elem.Valuer(),
	})
}

type ValueParseError struct {
	TokenType int
	Value     string
	Err       error
}

func (e ValueParseError) Error() string {
	return fmt.Sprintf("parsing %s value %q: %v", valueTokenString(e.TokenType), e.Value, e.Err)
}

//line parser.y:230
type ruleSymType struct {
	yys          int
	rule         Rule
	valueLiteral []byte
	operator     int
	negate       bool
	arrayValue   []valueToken
	valueToken   valueToken
}

const token_FIELD = 57346
const token_STRING = 57347
const token_HEX_STRING = 57348
const token_INT = 57349
const token_FLOAT = 57350
const token_BOOL = 57351
const token_IP_CIDR = 57352
const token_IP = 57353
const token_REGEX = 57354
const op_NOT = 57355
const op_AND = 57356
const op_OR = 57357
const token_LPAREN = 57358
const token_RPAREN = 57359
const token_LBRACKET = 57360
const token_RBRACKET = 57361
const token_COMMA = 57362
const op_EQ = 57363
const op_NE = 57364
const op_GT = 57365
const op_GE = 57366
const op_LT = 57367
const op_LE = 57368
const op_CONTAINS = 57369
const op_MATCHES = 57370
const op_IN = 57371
const token_ARRAY = 57372
const token_ERROR = 57373

var ruleToknames = [...]string{
	"$end",
	"error",
	"$unk",
	"token_FIELD",
	"token_STRING",
	"token_HEX_STRING",
	"token_INT",
	"token_FLOAT",
	"token_BOOL",
	"token_IP_CIDR",
	"token_IP",
	"token_REGEX",
	"op_NOT",
	"op_AND",
	"op_OR",
	"token_LPAREN",
	"token_RPAREN",
	"token_LBRACKET",
	"token_RBRACKET",
	"token_COMMA",
	"op_EQ",
	"op_NE",
	"op_GT",
	"op_GE",
	"op_LT",
	"op_LE",
	"op_CONTAINS",
	"op_MATCHES",
	"op_IN",
	"token_ARRAY",
	"token_ERROR",
}

var ruleStatenames = [...]string{}

const ruleEofCode = 1
const ruleErrCode = 2
const ruleInitialStackSize = 16

//line parser.y:485

//line yacctab:1
var ruleExca = [...]int8{
	-1, 1,
	1, -1,
	-2, 0,
	-1, 5,
	1, 9,
	14, 9,
	15, 9,
	17, 9,
	-2, 18,
}

const rulePrivate = 57344

const ruleLast = 60

var ruleAct = [...]int8{
	30, 23, 24, 19, 20, 21, 22, 25, 17, 18,
	33, 37, 27, 28, 34, 36, 35, 38, 45, 44,
	31, 39, 32, 39, 33, 37, 27, 28, 34, 36,
	35, 38, 6, 7, 5, 14, 6, 7, 26, 41,
	43, 7, 11, 3, 40, 46, 4, 1, 27, 28,
	29, 8, 9, 42, 12, 13, 10, 16, 15, 2,
}

var rulePact = [...]int16{
	30, 22, -1000, 30, 30, 29, 30, 30, -1000, 18,
	-20, -1000, 26, -1000, -1000, 41, 5, 32, 3, -1000,
	-1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000,
	-1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000, -1000, 19,
	-1000, -1000, -1, -1000, 19, -1000, -1000,
}

var rulePgo = [...]int8{
	0, 47, 59, 58, 57, 56, 53, 0, 22, 20,
	50,
}

var ruleR1 = [...]int8{
	0, 1, 1, 1, 1, 1, 2, 2, 2, 2,
	2, 3, 3, 3, 3, 4, 4, 4, 5, 5,
	6, 6, 9, 10, 10, 7, 7, 7, 7, 7,
	7, 7, 8, 8,
}

var ruleR2 = [...]int8{
	0, 1, 3, 3, 2, 3, 4, 4, 4, 1,
	4, 1, 1, 1, 1, 1, 1, 1, 0, 1,
	1, 3, 3, 1, 1, 1, 1, 1, 1, 1,
	1, 1, 1, 1,
}

var ruleChk = [...]int16{
	-1000, -1, -2, 13, 16, 4, 14, 15, -1, -1,
	-5, 13, -1, -1, 17, -3, -4, 28, 29, 23,
	24, 25, 26, 21, 22, 27, -8, 7, 8, -10,
	-7, -9, -8, 5, 9, 11, 10, 6, 12, 18,
	12, -9, -6, -7, 20, 19, -7,
}

var ruleDef = [...]int8{
	0, -2, 1, 0, 0, -2, 0, 0, 4, 0,
	0, 19, 2, 3, 5, 0, 0, 0, 0, 11,
	12, 13, 14, 15, 16, 17, 6, 32, 33, 7,
	23, 24, 25, 26, 27, 28, 29, 30, 31, 0,
	8, 10, 0, 20, 0, 22, 21,
}

var ruleTok1 = [...]int8{
	1,
}

var ruleTok2 = [...]int8{
	2, 3, 4, 5, 6, 7, 8, 9, 10, 11,
	12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
	22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
}

var ruleTok3 = [...]int8{
	0,
}

var ruleErrorMessages = [...]struct {
	state int
	token int
	msg   string
}{}

//line yaccpar:1

/*	parser for yacc output	*/

var (
	ruleDebug        = 0
	ruleErrorVerbose = false
)

type ruleLexer interface {
	Result(n Rule)
	Lex(lval *ruleSymType) int
	Error(s string)
}

type ruleParser interface {
	Parse(ruleLexer) int
	Lookahead() int
}

type ruleParserImpl struct {
	lval  ruleSymType
	stack [ruleInitialStackSize]ruleSymType
	char  int
}

func (p *ruleParserImpl) Lookahead() int {
	return p.char
}

func ruleNewParser() ruleParser {
	return &ruleParserImpl{}
}

const ruleFlag = -1000

func ruleTokname(c int) string {
	if c >= 1 && c-1 < len(ruleToknames) {
		if ruleToknames[c-1] != "" {
			return ruleToknames[c-1]
		}
	}
	return __yyfmt__.Sprintf("tok-%v", c)
}

func ruleStatname(s int) string {
	if s >= 0 && s < len(ruleStatenames) {
		if ruleStatenames[s] != "" {
			return ruleStatenames[s]
		}
	}
	return __yyfmt__.Sprintf("state-%v", s)
}

func ruleErrorMessage(state, lookAhead int) string {
	const TOKSTART = 4

	if !ruleErrorVerbose {
		return "syntax error"
	}

	for _, e := range ruleErrorMessages {
		if e.state == state && e.token == lookAhead {
			return "syntax error: " + e.msg
		}
	}

	res := "syntax error: unexpected " + ruleTokname(lookAhead)

	// To match Bison, suggest at most four expected tokens.
	expected := make([]int, 0, 4)

	// Look for shiftable tokens.
	base := int(rulePact[state])
	for tok := TOKSTART; tok-1 < len(ruleToknames); tok++ {
		if n := base + tok; n >= 0 && n < ruleLast && int(ruleChk[int(ruleAct[n])]) == tok {
			if len(expected) == cap(expected) {
				return res
			}
			expected = append(expected, tok)
		}
	}

	if ruleDef[state] == -2 {
		i := 0
		for ruleExca[i] != -1 || int(ruleExca[i+1]) != state {
			i += 2
		}

		// Look for tokens that we accept or reduce.
		for i += 2; ruleExca[i] >= 0; i += 2 {
			tok := int(ruleExca[i])
			if tok < TOKSTART || ruleExca[i+1] == 0 {
				continue
			}
			if len(expected) == cap(expected) {
				return res
			}
			expected = append(expected, tok)
		}

		// If the default action is to accept or reduce, give up.
		if ruleExca[i+1] != 0 {
			return res
		}
	}

	for i, tok := range expected {
		if i == 0 {
			res += ", expecting "
		} else {
			res += " or "
		}
		res += ruleTokname(tok)
	}
	return res
}

func rulelex1(lex ruleLexer, lval *ruleSymType) (char, token int) {
	token = 0
	char = lex.Lex(lval)
	if char <= 0 {
		token = int(ruleTok1[0])
		goto out
	}
	if char < len(ruleTok1) {
		token = int(ruleTok1[char])
		goto out
	}
	if char >= rulePrivate {
		if char < rulePrivate+len(ruleTok2) {
			token = int(ruleTok2[char-rulePrivate])
			goto out
		}
	}
	for i := 0; i < len(ruleTok3); i += 2 {
		token = int(ruleTok3[i+0])
		if token == char {
			token = int(ruleTok3[i+1])
			goto out
		}
	}

out:
	if token == 0 {
		token = int(ruleTok2[1]) /* unknown char */
	}
	if ruleDebug >= 3 {
		__yyfmt__.Printf("lex %s(%d)\n", ruleTokname(token), uint(char))
	}
	return char, token
}

func ruleParse(rulelex ruleLexer) int {
	return ruleNewParser().Parse(rulelex)
}

func (rulercvr *ruleParserImpl) Parse(rulelex ruleLexer) int {
	var rulen int
	var ruleVAL ruleSymType
	var ruleDollar []ruleSymType
	_ = ruleDollar // silence set and not used
	ruleS := rulercvr.stack[:]

	Nerrs := 0   /* number of errors */
	Errflag := 0 /* error recovery flag */
	rulestate := 0
	rulercvr.char = -1
	ruletoken := -1 // rulercvr.char translated into internal numbering
	defer func() {
		// Make sure we report no lookahead when not parsing.
		rulestate = -1
		rulercvr.char = -1
		ruletoken = -1
	}()
	rulep := -1
	goto rulestack

ret0:
	return 0

ret1:
	return 1

rulestack:
	/* put a state and value onto the stack */
	if ruleDebug >= 4 {
		__yyfmt__.Printf("char %v in %v\n", ruleTokname(ruletoken), ruleStatname(rulestate))
	}

	rulep++
	if rulep >= len(ruleS) {
		nyys := make([]ruleSymType, len(ruleS)*2)
		copy(nyys, ruleS)
		ruleS = nyys
	}
	ruleS[rulep] = ruleVAL
	ruleS[rulep].yys = rulestate

rulenewstate:
	rulen = int(rulePact[rulestate])
	if rulen <= ruleFlag {
		goto ruledefault /* simple state */
	}
	if rulercvr.char < 0 {
		rulercvr.char, ruletoken = rulelex1(rulelex, &rulercvr.lval)
	}
	rulen += ruletoken
	if rulen < 0 || rulen >= ruleLast {
		goto ruledefault
	}
	rulen = int(ruleAct[rulen])
	if int(ruleChk[rulen]) == ruletoken { /* valid shift */
		rulercvr.char = -1
		ruletoken = -1
		ruleVAL = rulercvr.lval
		rulestate = rulen
		if Errflag > 0 {
			Errflag--
		}
		goto rulestack
	}

ruledefault:
	/* default state action */
	rulen = int(ruleDef[rulestate])
	if rulen == -2 {
		if rulercvr.char < 0 {
			rulercvr.char, ruletoken = rulelex1(rulelex, &rulercvr.lval)
		}

		/* look through exception table */
		xi := 0
		for {
			if ruleExca[xi+0] == -1 && int(ruleExca[xi+1]) == rulestate {
				break
			}
			xi += 2
		}
		for xi += 2; ; xi += 2 {
			rulen = int(ruleExca[xi+0])
			if rulen < 0 || rulen == ruletoken {
				break
			}
		}
		rulen = int(ruleExca[xi+1])
		if rulen < 0 {
			goto ret0
		}
	}
	if rulen == 0 {
		/* error ... attempt to resume parsing */
		switch Errflag {
		case 0: /* brand new error */
			rulelex.Error(ruleErrorMessage(rulestate, ruletoken))
			Nerrs++
			if ruleDebug >= 1 {
				__yyfmt__.Printf("%s", ruleStatname(rulestate))
				__yyfmt__.Printf(" saw %s\n", ruleTokname(ruletoken))
			}
			fallthrough

		case 1, 2: /* incompletely recovered error ... try again */
			Errflag = 3

			/* find a state where "error" is a legal shift action */
			for rulep >= 0 {
				rulen = int(rulePact[ruleS[rulep].yys]) + ruleErrCode
				if rulen >= 0 && rulen < ruleLast {
					rulestate = int(ruleAct[rulen]) /* simulate a shift of "error" */
					if int(ruleChk[rulestate]) == ruleErrCode {
						goto rulestack
					}
				}

				/* the current p has no shift on "error", pop stack */
				if ruleDebug >= 2 {
					__yyfmt__.Printf("error recovery pops state %d\n", ruleS[rulep].yys)
				}
				rulep--
			}
			/* there is no state on the stack with an error shift ... abort */
			goto ret1

		case 3: /* no shift yet; clobber input char */
			if ruleDebug >= 2 {
				__yyfmt__.Printf("error recovery discards %s\n", ruleTokname(ruletoken))
			}
			if ruletoken == ruleEofCode {
				goto ret1
			}
			rulercvr.char = -1
			ruletoken = -1
			goto rulenewstate /* try again in the same state */
		}
	}

	/* reduction by production rulen */
	if ruleDebug >= 2 {
		__yyfmt__.Printf("reduce %v in:\n\t%v\n", rulen, ruleStatname(rulestate))
	}

	rulent := rulen
	rulept := rulep
	_ = rulept // guard against "declared and not used"

	rulep -= int(ruleR2[rulen])
	// rulep is now the index of $0. Perform the default action. Iff the
	// reduced production is ε, $1 is possibly out of range.
	if rulep+1 >= len(ruleS) {
		nyys := make([]ruleSymType, len(ruleS)*2)
		copy(nyys, ruleS)
		ruleS = nyys
	}
	ruleVAL = ruleS[rulep+1]

	/* consult goto table to find next state */
	rulen = int(ruleR1[rulen])
	ruleg := int(rulePgo[rulen])
	rulej := ruleg + ruleS[rulep].yys + 1

	if rulej >= ruleLast {
		rulestate = int(ruleAct[ruleg])
	} else {
		rulestate = int(ruleAct[rulej])
		if int(ruleChk[rulestate]) != -rulen {
			rulestate = int(ruleAct[ruleg])
		}
	}
	// dummy call; replaced with literal code
	switch rulent {

	case 1:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:277
		{
			ruleVAL.rule = ruleDollar[1].rule
			rulelex.Result(ruleVAL.rule)
		}
	case 2:
		ruleDollar = ruleS[rulept-3 : rulept+1]
//line parser.y:282
		{
			ruleVAL.rule = &nodeAnd{left: ruleDollar[1].rule, right: ruleDollar[3].rule}
			rulelex.Result(ruleVAL.rule)
		}
	case 3:
		ruleDollar = ruleS[rulept-3 : rulept+1]
//line parser.y:287
		{
			ruleVAL.rule = &nodeOr{left: ruleDollar[1].rule, right: ruleDollar[3].rule}
			rulelex.Result(ruleVAL.rule)
		}
	case 4:
		ruleDollar = ruleS[rulept-2 : rulept+1]
//line parser.y:292
		{
			ruleVAL.rule = &nodeNot{right: ruleDollar[2].rule}
			rulelex.Result(ruleVAL.rule)
		}
	case 5:
		ruleDollar = ruleS[rulept-3 : rulept+1]
//line parser.y:297
		{
			ruleVAL.rule = ruleDollar[2].rule
			rulelex.Result(ruleVAL.rule)
		}
	case 6:
		ruleDollar = ruleS[rulept-4 : rulept+1]
//line parser.y:306
		{
			ruleVAL.rule = makeCompareNode(string(ruleDollar[1].valueLiteral), ruleDollar[2].negate, ruleDollar[3].operator, ruleDollar[4].valueToken)
		}
	case 7:
		ruleDollar = ruleS[rulept-4 : rulept+1]
//line parser.y:311
		{
			ruleVAL.rule = makeCompareNode(string(ruleDollar[1].valueLiteral), ruleDollar[2].negate, ruleDollar[3].operator, ruleDollar[4].valueToken)
		}
	case 8:
		ruleDollar = ruleS[rulept-4 : rulept+1]
//line parser.y:316
		{
			elem, err := newValueToken(token_REGEX, ruleDollar[4].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}

			ruleVAL.rule = withNegate(ruleDollar[2].negate, &nodeMatch{
				lv: FieldValue(string(ruleDollar[1].valueLiteral)),
				rv: elem.Valuer(),
			})
		}
	case 9:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:329
		{
			ruleVAL.rule = &nodeNotZero{FieldValue(string(ruleDollar[1].valueLiteral))}
		}
	case 10:
		ruleDollar = ruleS[rulept-4 : rulept+1]
//line parser.y:334
		{
			values, ok := ruleDollar[4].valueToken.value.([]any)
			if !ok {
				rulelex.Error(fmt.Errorf("parser error while handling array value %q", ruleDollar[4].valueToken.raw).Error())
				return 1
			}

			ruleVAL.rule = withNegate(ruleDollar[2].negate, &nodeIn{
				lv: FieldValue(string(ruleDollar[1].valueLiteral)),
				rv: LiteralValue[[]any]{
					raw:   ruleDollar[4].valueToken.raw,
					value: values,
				},
			})
		}
	case 11:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:352
		{
			ruleVAL.operator = op_GT
		}
	case 12:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:353
		{
			ruleVAL.operator = op_GE
		}
	case 13:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:354
		{
			ruleVAL.operator = op_LT
		}
	case 14:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:355
		{
			ruleVAL.operator = op_LE
		}
	case 15:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:359
		{
			ruleVAL.operator = op_EQ
		}
	case 16:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:360
		{
			ruleVAL.operator = op_NE
		}
	case 17:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:361
		{
			ruleVAL.operator = op_CONTAINS
		}
	case 18:
		ruleDollar = ruleS[rulept-0 : rulept+1]
//line parser.y:365
		{
			ruleVAL.negate = false
		}
	case 19:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:366
		{
			ruleVAL.negate = true
		}
	case 20:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:372
		{
			ruleVAL.arrayValue = []valueToken{ruleDollar[1].valueToken}
		}
	case 21:
		ruleDollar = ruleS[rulept-3 : rulept+1]
//line parser.y:376
		{
			ruleVAL.arrayValue = append(ruleDollar[1].arrayValue, ruleDollar[3].valueToken)
		}
	case 22:
		ruleDollar = ruleS[rulept-3 : rulept+1]
//line parser.y:383
		{
			raw_parts := make([]string, len(ruleDollar[2].arrayValue))
			values := make([]any, len(ruleDollar[2].arrayValue))
			for i, elem := range ruleDollar[2].arrayValue {
				raw_parts[i] = elem.raw
				values[i] = elem.value
			}
			raw := fmt.Sprintf("[%s]", strings.Join(raw_parts, ", "))

			ruleVAL.valueToken = valueToken{
				typ:   token_ARRAY,
				raw:   raw,
				value: values,
			}
		}
	case 23:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:401
		{
			ruleVAL.valueToken = ruleDollar[1].valueToken
		}
	case 24:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:402
		{
			ruleVAL.valueToken = ruleDollar[1].valueToken
		}
	case 25:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:407
		{
			ruleVAL.valueToken = ruleDollar[1].valueToken
		}
	case 26:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:409
		{
			v, err := newValueToken(token_STRING, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 27:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:418
		{
			v, err := newValueToken(token_BOOL, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 28:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:427
		{
			v, err := newValueToken(token_IP, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 29:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:436
		{
			v, err := newValueToken(token_IP_CIDR, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 30:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:445
		{
			v, err := newValueToken(token_HEX_STRING, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 31:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:454
		{
			v, err := newValueToken(token_REGEX, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 32:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:466
		{
			v, err := newValueToken(token_INT, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	case 33:
		ruleDollar = ruleS[rulept-1 : rulept+1]
//line parser.y:475
		{
			v, err := newValueToken(token_FLOAT, ruleDollar[1].valueLiteral)
			if err != nil {
				rulelex.Error(err.Error())
				return 1
			}
			ruleVAL.valueToken = v
		}
	}
	goto rulestack /* stack new state and value */
}
