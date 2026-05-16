package rulekit

// Macro is a named zero-argument rule expression that can be reused by calls.
type Macro struct {
	Name   string
	Source string
	AST    *AST
	Rule   Rule
	Doc    string
}

type MacroSet map[string]*Macro

func NewMacro(name string, source string) (*Macro, error) {
	ast, err := ParseAST(source)
	if err != nil {
		return nil, err
	}
	rule, err := Compile(ast)
	if err != nil {
		return nil, err
	}
	return &Macro{Name: name, Source: source, AST: ast, Rule: rule}, nil
}

func MustMacro(name string, source string) *Macro {
	macro, err := NewMacro(name, source)
	if err != nil {
		panic(err)
	}
	return macro
}
