package rulekit

// Macro is a named zero-argument rule expression that can be reused by calls.
type Macro struct {
	Source string
	AST    *AST
	Rule   Rule
	Doc    string
}

type MacroSet map[string]*Macro

func NewMacro(source string) (*Macro, error) {
	ast, err := ParseAST(source)
	if err != nil {
		return nil, err
	}
	rule, err := Compile(ast)
	if err != nil {
		return nil, err
	}
	return &Macro{Source: source, AST: ast, Rule: rule}, nil
}

func MustMacro(source string) *Macro {
	macro, err := NewMacro(source)
	if err != nil {
		panic(err)
	}
	return macro
}

func (m *MacroSet) Register(name string, source string) error {
	macro, err := NewMacro(source)
	if err != nil {
		return err
	}
	if *m == nil {
		*m = MacroSet{}
	}
	(*m)[name] = macro
	return nil
}
