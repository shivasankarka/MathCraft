//! MathCraft's own syntax tree: the subset of Python the language supports. Statements carry their line number
//! (1-based, within the cell) so runtime errors can point at the right place.

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Expr(Ex),
    /// `a = b = value`
    Assign {
        targets: Vec<Target>,
        value: Ex,
    },
    AugAssign {
        target: Target,
        op: BinOp,
        value: Ex,
    },
    Def {
        name: String,
        params: Vec<Param>,
        body: Vec<Stmt>,
    },
    Return(Option<Ex>),
    /// `elif` chains are lowered to nested `If`s in `orelse`.
    If {
        test: Ex,
        body: Vec<Stmt>,
        orelse: Vec<Stmt>,
    },
    For {
        target: Target,
        iter: Ex,
        body: Vec<Stmt>,
    },
    While {
        test: Ex,
        body: Vec<Stmt>,
    },
    Pass,
    Break,
    Continue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Name(String),
    Tuple(Vec<Target>),
}

impl Target {
    pub fn names(&self, out: &mut Vec<String>) {
        match self {
            Target::Name(n) => out.push(n.clone()),
            Target::Tuple(ts) => ts.iter().for_each(|t| t.names(out)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub default: Option<Ex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    MatMul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    In,
    NotIn,
    Is,
    IsNot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comp {
    pub target: Target,
    pub iter: Ex,
    pub ifs: Vec<Ex>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ex {
    /// Integer literal kept as text so arbitrarily large values survive.
    Int(String),
    Float(f64),
    Str(String),
    Bool(bool),
    None,
    Name(String),
    Bin(Box<Ex>, BinOp, Box<Ex>),
    Unary(UnOp, Box<Ex>),
    Compare(Box<Ex>, Vec<(CmpOp, Ex)>),
    And(Vec<Ex>),
    Or(Vec<Ex>),
    Call {
        func: Box<Ex>,
        args: Vec<Ex>,
        kwargs: Vec<(String, Ex)>,
    },
    Attr(Box<Ex>, String),
    Index(Box<Ex>, Box<Ex>),
    /// `lower:upper:step`, only ever the second operand of an `Index`.
    Slice(Option<Box<Ex>>, Option<Box<Ex>>, Option<Box<Ex>>),
    List(Vec<Ex>),
    Tuple(Vec<Ex>),
    Lambda {
        params: Vec<Param>,
        body: Box<Ex>,
    },
    ListComp {
        elt: Box<Ex>,
        gens: Vec<Comp>,
    },
    IfExp {
        test: Box<Ex>,
        body: Box<Ex>,
        orelse: Box<Ex>,
    },
}
