//! Types and the definitions they refer to.

use crate::source::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    /// The type of something that already has an error; accepted everywhere
    /// so one mistake doesn't cause a cascade of follow-on errors.
    Error,
    Void,
    Int,
    Float,
    Bool,
    Str,
    /// Raw bytes (`Data`): files, and the binary encoding of values.
    Data,
    Range,
    View,
    List(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Opt(Box<Ty>),
    Fn(Vec<Ty>, Box<Ty>),
    Struct(usize),
    Enum(usize),
    /// Only in built-in signatures: accepts any value (checked specially).
    Any,
}

impl Ty {
    pub fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }

    pub fn opt(t: Ty) -> Ty {
        match t {
            Ty::Opt(_) | Ty::Error => t,
            t => Ty::Opt(Box::new(t)),
        }
    }

    pub fn func(params: Vec<Ty>, ret: Ty) -> Ty {
        Ty::Fn(params, Box::new(ret))
    }

    pub fn is_error(&self) -> bool {
        *self == Ty::Error
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int | Ty::Float | Ty::Error)
    }

    /// Does this type contain an error anywhere (so mismatches aren't reported)?
    pub fn has_error(&self) -> bool {
        match self {
            Ty::Error => true,
            Ty::List(t) | Ty::Opt(t) => t.has_error(),
            Ty::Dict(k, v) => k.has_error() || v.has_error(),
            Ty::Fn(ps, r) => ps.iter().any(Ty::has_error) || r.has_error(),
            _ => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParamSig {
    pub label: String,
    pub unlabeled: bool,
    pub ty: Ty,
    pub has_default: bool,
    pub bind: bool,
    /// Picker's `selection`: its type becomes the type `.tag(…)` expects.
    pub sets_tag: bool,
    /// A `fn(T)` whose `T` is that tag type (TabView's `onClose`).
    pub tag_fn: bool,
}

impl ParamSig {
    pub fn new(label: &str, ty: Ty) -> ParamSig {
        ParamSig { label: label.into(), unlabeled: false, ty, has_default: false, bind: false, sets_tag: false, tag_fn: false }
    }

    /// `_ label: T`
    pub fn unlabeled(label: &str, ty: Ty) -> ParamSig {
        ParamSig { unlabeled: true, ..ParamSig::new(label, ty) }
    }

    pub fn default(mut self) -> ParamSig {
        self.has_default = true;
        self
    }

    pub fn bind(mut self) -> ParamSig {
        self.bind = true;
        self
    }
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub params: Vec<ParamSig>,
    pub ret: Ty,
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct MethodDef {
    pub name: String,
    pub sig: FnSig,
    pub mutating: bool,
    pub span: Span,
}

#[derive(Debug)]
pub struct StructDef {
    pub name: String,
    pub span: Span,
    pub fields: Vec<FieldDef>,
    pub methods: Vec<MethodDef>,
}

#[derive(Clone, Debug)]
pub struct CaseDef {
    pub name: String,
    pub fields: Vec<ParamSig>,
}

#[derive(Debug)]
pub struct EnumDef {
    pub name: String,
    /// `None` for built-in enums like `Color`.
    pub span: Option<Span>,
    pub cases: Vec<CaseDef>,
    pub methods: Vec<MethodDef>,
}

impl EnumDef {
    pub fn case(&self, name: &str) -> Option<&CaseDef> {
        self.cases.iter().find(|c| c.name == name)
    }
}
