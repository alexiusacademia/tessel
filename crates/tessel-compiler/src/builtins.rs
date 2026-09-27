//! Signatures of the built-in views, modifiers, functions and methods.

use crate::types::{CaseDef, EnumDef, FnSig, ParamSig as P, Ty};

pub const COLORS: &[&str] = &[
    "primary", "secondary", "gray", "red", "orange", "yellow", "green", "blue", "purple", "pink", "white", "black",
    "clear", "lightGray", "sidebar", "highlight", "separator",
];

pub const WEIGHTS: &[&str] = &["regular", "medium", "semibold", "bold"];

pub const ALIGNMENTS: &[&str] = &["leading", "center", "trailing"];

/// Built-in enums, registered before user types: Color, Weight, Alignment.
pub fn enums() -> Vec<EnumDef> {
    let simple = |name: &str, cases: &[&str]| EnumDef {
        name: name.into(),
        span: None,
        cases: cases.iter().map(|c| CaseDef { name: (*c).into(), fields: Vec::new() }).collect(),
        methods: Vec::new(),
    };
    vec![simple("Color", COLORS), simple("Weight", WEIGHTS), simple("Alignment", ALIGNMENTS)]
}

pub struct Builtins {
    pub color: Ty,
    pub weight: Ty,
    pub alignment: Ty,
}

pub const VIEW_NAMES: &[&str] =
    &[
        "Text", "Button", "TextField", "Toggle", "Picker", "VStack", "HStack", "ZStack", "Spacer", "Scroll", "Image",
        "CodeEditor", "TabView", "Divider",
    ];

impl Builtins {
    pub fn view(&self, name: &str) -> Option<Vec<P>> {
        let content = || P::new("content", Ty::View);
        Some(match name {
            "Text" => vec![P::unlabeled("text", Ty::Str)],
            "Button" => vec![P::unlabeled("label", Ty::Str), P::new("action", Ty::func(vec![], Ty::Void))],
            "TextField" => vec![P::unlabeled("placeholder", Ty::Str), P::new("text", Ty::Str).bind()],
            "Toggle" => vec![P::unlabeled("label", Ty::Str).default(), P::new("isOn", Ty::Bool).bind()],
            "CodeEditor" => vec![
                P::new("text", Ty::Str).bind(),
                P::new("errorLines", Ty::list(Ty::Int)).default(),
                P::new("line", Ty::Int).bind().default(),
            ],
            "Picker" => {
                let mut selection = P::new("selection", Ty::Any).bind();
                selection.sets_tag = true;
                vec![selection, content()]
            }
            "TabView" => {
                let mut selection = P::new("selection", Ty::Any).bind();
                selection.sets_tag = true;
                // `onClose: fn(T)`, where T is the selection's type.
                let mut on_close = P::new("onClose", Ty::Any).default();
                on_close.tag_fn = true;
                vec![selection, on_close, content()]
            }
            "Divider" => vec![],
            "VStack" | "HStack" => vec![
                P::new("spacing", Ty::Float).default(),
                P::new("alignment", self.alignment.clone()).default(),
                content(),
            ],
            "ZStack" | "Scroll" => vec![content()],
            "Spacer" => vec![],
            "Image" => vec![P::unlabeled("path", Ty::Str)],
            _ => return None,
        })
    }

    /// Modifiers callable on any `View`. `id` and `tag` are checked specially.
    pub fn modifier(&self, name: &str) -> Option<Vec<P>> {
        Some(match name {
            "padding" => vec![P::unlabeled("amount", Ty::Float).default()],
            "font" => vec![P::new("size", Ty::Float).default(), P::new("weight", self.weight.clone()).default()],
            "color" => vec![P::unlabeled("color", self.color.clone())],
            "background" => vec![P::unlabeled("color", self.color.clone()), P::new("radius", Ty::Float).default()],
            "border" => vec![
                P::unlabeled("color", self.color.clone()),
                P::new("width", Ty::Float).default(),
                P::new("radius", Ty::Float).default(),
            ],
            "frame" => {
                vec![P::new("width", Ty::opt(Ty::Float)).default(), P::new("height", Ty::opt(Ty::Float)).default()]
            }
            "opacity" => vec![P::unlabeled("value", Ty::Float)],
            "hidden" => vec![P::unlabeled("isHidden", Ty::Bool).default()],
            "disabled" => vec![P::unlabeled("isDisabled", Ty::Bool).default()],
            "onTap" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "id" | "tag" => vec![P::unlabeled("value", Ty::Any)],
            "shortcut" => vec![P::unlabeled("key", Ty::Str), P::new("action", Ty::func(vec![], Ty::Void))],
            "onAppear" | "onSubmit" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "tabItem" => vec![P::unlabeled("title", Ty::Str)],
            "autofocus" => vec![],
            "hoverBackground" => vec![P::unlabeled("color", self.color.clone()), P::new("radius", Ty::Float).default()],
            _ => return None,
        })
    }

    pub fn function(&self, name: &str) -> Option<FnSig> {
        Some(match name {
            "print" => FnSig { params: vec![P::unlabeled("value", Ty::Any)], ret: Ty::Void },
            "fetch" => FnSig {
                params: vec![P::new("url", Ty::Str), P::new("done", Ty::func(vec![Ty::opt(Ty::Str)], Ty::Void))],
                ret: Ty::Void,
            },
            "after" => FnSig {
                params: vec![P::new("seconds", Ty::Float), P::new("action", Ty::func(vec![], Ty::Void))],
                ret: Ty::Void,
            },
            "readFile" => FnSig { params: vec![P::new("path", Ty::Str)], ret: Ty::opt(Ty::Str) },
            "writeFile" => FnSig { params: vec![P::new("path", Ty::Str), P::new("text", Ty::Str)], ret: Ty::Bool },
            "listFolder" => FnSig { params: vec![P::new("path", Ty::Str)], ret: Ty::opt(Ty::list(Ty::Str)) },
            "fileExists" | "isFolder" | "deleteFile" | "createFolder" => {
                FnSig { params: vec![P::new("path", Ty::Str)], ret: Ty::Bool }
            }
            "renameFile" => FnSig { params: vec![P::new("from", Ty::Str), P::new("to", Ty::Str)], ret: Ty::Bool },
            "currentFolder" => FnSig { params: vec![], ret: Ty::Str },
            "environment" => FnSig { params: vec![P::new("name", Ty::Str)], ret: Ty::opt(Ty::Str) },
            "runCommand" => FnSig {
                params: vec![
                    P::new("path", Ty::Str),
                    P::new("args", Ty::list(Ty::Str)).default(),
                    P::new("onOutput", Ty::func(vec![Ty::Str], Ty::Void)),
                    P::new("onExit", Ty::func(vec![Ty::Int], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "stopCommand" => FnSig { params: vec![P::new("id", Ty::Int)], ret: Ty::Void },
            _ => return None,
        })
    }
}

pub const FUNCTION_NAMES: &[&str] = &[
    "print", "fetch", "after", "Int", "Float", "String", "readFile", "writeFile", "listFolder", "fileExists",
    "isFolder", "deleteFile", "createFolder", "renameFile", "currentFolder", "environment", "runCommand",
    "stopCommand",
];

/// A built-in method: its signature and whether it changes the receiver.
pub struct Method {
    pub sig: FnSig,
    pub mutating: bool,
}

fn method(params: Vec<P>, ret: Ty) -> Method {
    Method { sig: FnSig { params, ret }, mutating: false }
}

fn mutating(params: Vec<P>, ret: Ty) -> Method {
    Method { sig: FnSig { params, ret }, mutating: true }
}

/// Properties (no call) of built-in types.
pub fn property(recv: &Ty, name: &str) -> Option<Ty> {
    Some(match (recv, name) {
        (Ty::List(_) | Ty::Str | Ty::Dict(..), "count") => Ty::Int,
        (Ty::List(_) | Ty::Str | Ty::Dict(..), "isEmpty") => Ty::Bool,
        (Ty::List(t), "first" | "last") => Ty::opt((**t).clone()),
        (Ty::Dict(k, _), "keys") => Ty::list((**k).clone()),
        (Ty::Dict(_, v), "values") => Ty::list((**v).clone()),
        _ => return None,
    })
}

/// Methods of built-in types. `map` and `sorted` are checked specially.
pub fn method_of(recv: &Ty, name: &str) -> Option<Method> {
    let pred = |t: &Ty| Ty::func(vec![t.clone()], Ty::Bool);
    Some(match recv {
        Ty::List(t) => {
            let t = (**t).clone();
            match name {
                "append" => mutating(vec![P::unlabeled("item", t)], Ty::Void),
                "insert" => mutating(vec![P::unlabeled("item", t), P::new("at", Ty::Int)], Ty::Void),
                "remove" => mutating(vec![P::new("at", Ty::Int)], t),
                "removeAll" => mutating(vec![P::new("where", pred(&t))], Ty::Void),
                "filter" => method(vec![P::unlabeled("keep", pred(&t))], Ty::list(t)),
                "first" | "last" => method(vec![P::new("where", pred(&t))], Ty::opt(t)),
                "contains" => method(vec![P::unlabeled("item", t)], Ty::Bool),
                "reversed" => method(vec![], Ty::list(t)),
                "map" => method(vec![P::unlabeled("transform", Ty::Any)], Ty::Any),
                "sorted" => method(
                    vec![P::new("by", Ty::func(vec![t.clone(), t.clone()], Ty::Bool)).default()],
                    Ty::list(t),
                ),
                "joined" if t == Ty::Str => method(vec![P::new("separator", Ty::Str).default()], Ty::Str),
                _ => return None,
            }
        }
        Ty::Str => match name {
            "contains" | "hasPrefix" | "hasSuffix" => method(vec![P::unlabeled("text", Ty::Str)], Ty::Bool),
            "split" => method(vec![P::unlabeled("separator", Ty::Str)], Ty::list(Ty::Str)),
            "trim" | "uppercase" | "lowercase" => method(vec![], Ty::Str),
            "replace" => method(vec![P::unlabeled("text", Ty::Str), P::new("with", Ty::Str)], Ty::Str),
            "lines" => method(vec![], Ty::list(Ty::Str)),
            "substring" => method(vec![P::new("from", Ty::Int), P::new("to", Ty::Int)], Ty::Str),
            "find" => method(vec![P::unlabeled("text", Ty::Str)], Ty::opt(Ty::Int)),
            _ => return None,
        },
        Ty::Dict(k, v) => match name {
            "removeValue" => mutating(vec![P::new("forKey", (**k).clone())], Ty::opt((**v).clone())),
            "contains" => method(vec![P::new("key", (**k).clone())], Ty::Bool),
            _ => return None,
        },
        _ => return None,
    })
}

/// Names shown in "did you mean" suggestions for members of built-in types.
pub fn member_names(recv: &Ty) -> &'static [&'static str] {
    match recv {
        Ty::List(_) => &[
            "count", "isEmpty", "first", "last", "append", "insert", "remove", "removeAll", "filter", "contains",
            "reversed", "map", "sorted", "joined",
        ],
        Ty::Str => &[
            "count", "isEmpty", "contains", "hasPrefix", "hasSuffix", "split", "trim", "uppercase", "lowercase",
            "replace", "lines", "substring", "find",
        ],
        Ty::Dict(..) => &["count", "isEmpty", "keys", "values", "removeValue", "contains"],
        Ty::View => &[
            "padding", "font", "color", "background", "border", "frame", "opacity", "hidden", "disabled", "onTap",
            "id", "tag", "shortcut", "onAppear", "onSubmit", "tabItem", "hoverBackground", "autofocus",
        ],
        _ => &[],
    }
}
