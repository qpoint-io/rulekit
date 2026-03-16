/// Comparison operators used in rule predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    #[allow(dead_code)]
    Matches,
    #[allow(dead_code)]
    In,
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operator::Eq => write!(f, "=="),
            Operator::Ne => write!(f, "!="),
            Operator::Gt => write!(f, ">"),
            Operator::Ge => write!(f, ">="),
            Operator::Lt => write!(f, "<"),
            Operator::Le => write!(f, "<="),
            Operator::Contains => write!(f, "contains"),
            Operator::Matches => write!(f, "matches"),
            Operator::In => write!(f, "in"),
        }
    }
}

/// A parsed literal value.
#[derive(Debug, Clone)]
pub enum LiteralValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Ip(std::net::IpAddr),
    IpCidr(String),
    HexString(Vec<u8>),
    Regex(String),
}

impl std::fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LiteralValue::String(s) => write!(f, "\"{}\"", s),
            LiteralValue::Int(n) => write!(f, "{}", n),
            LiteralValue::Float(n) => write!(f, "{}", n),
            LiteralValue::Bool(b) => write!(f, "{}", b),
            LiteralValue::Ip(ip) => write!(f, "{}", ip),
            LiteralValue::IpCidr(cidr) => write!(f, "{}", cidr),
            LiteralValue::HexString(bytes) => {
                let hex: Vec<String> = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                write!(f, "{}", hex.join(":"))
            }
            LiteralValue::Regex(r) => write!(f, "/{}/", r),
        }
    }
}

/// The AST produced by the parser. Each variant corresponds to a grammar rule.
#[derive(Debug, Clone)]
pub enum Expr {
    /// `left AND right`
    And {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `left OR right`
    Or {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `NOT expr`
    Not {
        expr: Box<Expr>,
    },
    /// `left op right` where op is an equality/inequality operator
    Compare {
        left: Box<Expr>,
        op: Operator,
        right: Box<Expr>,
    },
    /// `left matches right` where right is a regex literal
    Match {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `left in right` where right is an array or CIDR
    In {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A field reference, e.g. `http.host` or `status_code`
    Field(String),
    /// A parsed literal value (string, int, float, bool, IP, regex, etc.)
    Literal(LiteralValue),
    /// An array literal `[v1, v2, ...]`
    Array(Vec<Expr>),
    /// A function call `name(arg1, arg2, ...)`
    FunctionCall {
        name: String,
        args: Vec<Expr>,
    },
}

impl std::fmt::Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expr::And { left, right } => write!(f, "{} and {}", left, right),
            Expr::Or { left, right } => write!(f, "{} or {}", left, right),
            Expr::Not { expr } => write!(f, "not {}", expr),
            Expr::Compare { left, op, right } => write!(f, "{} {} {}", left, op, right),
            Expr::Match { left, right } => write!(f, "{} matches {}", left, right),
            Expr::In { left, right } => write!(f, "{} in {}", left, right),
            Expr::Field(name) => write!(f, "{}", name),
            Expr::Literal(val) => write!(f, "{}", val),
            Expr::Array(vals) => {
                write!(f, "[")?;
                for (i, v) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            }
            Expr::FunctionCall { name, args } => {
                write!(f, "{}(", name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", a)?;
                }
                write!(f, ")")
            }
        }
    }
}
