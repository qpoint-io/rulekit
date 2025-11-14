package rulekit

import "encoding/json"

type ASTNode interface {
	NodeType() string
}

type ASTNodeOperator struct {
	Operator string  `json:"operator"`
	Left     ASTNode `json:"left"`
	Right    ASTNode `json:"right"`
}

func (n *ASTNodeOperator) NodeType() string {
	return "operator"
}

func (n *ASTNodeOperator) MarshalJSON() ([]byte, error) {
	type Alias ASTNodeOperator
	return json.Marshal(&struct {
		NodeType string `json:"node_type"`
		*Alias
	}{
		NodeType: n.NodeType(),
		Alias:    (*Alias)(n),
	})
}

type ASTNodeField struct {
	Name string `json:"name"`
}

func (n *ASTNodeField) NodeType() string {
	return "field"
}

func (n *ASTNodeField) MarshalJSON() ([]byte, error) {
	type Alias ASTNodeField
	return json.Marshal(&struct {
		NodeType string `json:"node_type"`
		*Alias
	}{
		NodeType: n.NodeType(),
		Alias:    (*Alias)(n),
	})
}

type ASTNodeLiteral struct {
	Type  string `json:"type"`
	Value any    `json:"value"`
}

func (n *ASTNodeLiteral) NodeType() string {
	return "literal"
}

func (n *ASTNodeLiteral) MarshalJSON() ([]byte, error) {
	type Alias ASTNodeLiteral
	return json.Marshal(&struct {
		NodeType string `json:"node_type"`
		*Alias
	}{
		NodeType: n.NodeType(),
		Alias:    (*Alias)(n),
	})
}

type ASTNodeArray struct {
	Elements []ASTNode `json:"elements"`
}

func (n *ASTNodeArray) NodeType() string {
	return "array"
}

func (n *ASTNodeArray) MarshalJSON() ([]byte, error) {
	type Alias ASTNodeArray
	return json.Marshal(&struct {
		NodeType string `json:"node_type"`
		*Alias
	}{
		NodeType: n.NodeType(),
		Alias:    (*Alias)(n),
	})
}

type ASTNodeFunction struct {
	Name string        `json:"name"`
	Args *ASTNodeArray `json:"args"`
}

func (n *ASTNodeFunction) NodeType() string {
	return "function"
}

func (n *ASTNodeFunction) MarshalJSON() ([]byte, error) {
	type Alias ASTNodeFunction
	return json.Marshal(&struct {
		NodeType string `json:"node_type"`
		*Alias
	}{
		NodeType: n.NodeType(),
		Alias:    (*Alias)(n),
	})
}
