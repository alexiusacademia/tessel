//! Signatures of the built-in views, modifiers, functions and methods.

use crate::source::Span;
use crate::types::{CaseDef, EnumDef, FieldDef, FnSig, ParamSig as P, StructDef, Ty};

pub const COLORS: &[&str] = &[
    "primary", "secondary", "gray", "red", "orange", "yellow", "green", "blue", "purple", "pink", "white", "black",
    "clear", "lightGray", "sidebar", "highlight", "separator", "background",
];

/// How an app looks: like the system (the default), or always light or dark.
pub const APPEARANCES: &[&str] = &["system", "light", "dark"];

pub const WEIGHTS: &[&str] = &["regular", "medium", "semibold", "bold"];

/// In this order (the runtime relies on the tags): stacks use the first three
/// plus `top`/`bottom`; frames and ZStacks use all of them.
pub const ALIGNMENTS: &[&str] =
    &["leading", "center", "trailing", "top", "bottom", "topLeading", "topTrailing", "bottomLeading", "bottomTrailing"];

/// What a mouse did, for `.onPointer` (the runtime relies on the order).
pub const POINTER_KINDS: &[&str] = &["down", "up", "move", "drag", "scroll", "pinch", "leave"];

/// Mouse buttons (the runtime relies on the order).
pub const POINTER_BUTTONS: &[&str] = &["left", "right", "middle"];

/// Pointers for `.cursor(…)` (the runtime relies on the order).
pub const CURSOR_STYLES: &[&str] = &[
    "arrow", "hand", "crosshair", "grab", "grabbing", "text", "move", "wait", "notAllowed", "resizeHorizontal",
    "resizeVertical", "zoomIn", "zoomOut",
];

/// Names of the built-in icons, in order (shared with the runtime).
pub const ICON_NAMES: &str = include_str!("../../tessel-runtime/icons/names.txt");

/// Built-in enums, registered before user types: Color, Weight, Alignment,
/// IconName, Appearance, PointerKind, PointerButton, CursorStyle.
pub fn enums() -> Vec<EnumDef> {
    let simple = |name: &str, cases: &[&str]| EnumDef {
        name: name.into(),
        span: None,
        cases: cases.iter().map(|c| CaseDef { name: (*c).into(), fields: Vec::new() }).collect(),
        methods: Vec::new(),
    };
    let icons: Vec<&str> = ICON_NAMES.lines().map(str::trim).filter(|n| !n.is_empty()).collect();
    // After its named cases (the runtime relies on the order), `Color` has
    // `.rgb(red, green, blue)` (each 0 to 255) and `.rgba(…, opacity)` (0 to 1).
    let mut color = simple("Color", COLORS);
    let channels = || vec![P::unlabeled("red", Ty::Int), P::unlabeled("green", Ty::Int), P::unlabeled("blue", Ty::Int)];
    color.cases.push(CaseDef { name: "rgb".into(), fields: channels() });
    let mut with_opacity = channels();
    with_opacity.push(P::unlabeled("opacity", Ty::Float));
    color.cases.push(CaseDef { name: "rgba".into(), fields: with_opacity });
    vec![
        color,
        simple("Weight", WEIGHTS),
        simple("Alignment", ALIGNMENTS),
        simple("IconName", &icons),
        simple("Appearance", APPEARANCES),
        simple("PointerKind", POINTER_KINDS),
        simple("PointerButton", POINTER_BUTTONS),
        simple("CursorStyle", CURSOR_STYLES),
    ]
}

/// Built-in structs, registered before user types: `HttpResponse` is struct 0.
pub fn structs() -> Vec<StructDef> {
    let field = |name: &str, ty: Ty| FieldDef { name: name.into(), ty, has_default: false, span: Span::default() };
    vec![
        StructDef {
        name: "HttpResponse".into(),
        span: Span::default(),
        fields: vec![
            field("status", Ty::Int),
            field("ok", Ty::Bool),
            field("text", Ty::Str),
            field("data", Ty::Data),
            field("headers", Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Str))),
            field("error", Ty::Str),
        ],
        methods: Vec::new(),
        },
        // A `CodeEditor` suggestion: what to insert, and what to show next to it.
        StructDef {
            name: "Completion".into(),
            span: Span::default(),
            fields: vec![field("label", Ty::Str), field("detail", Ty::Str), field("kind", Ty::Str)],
            methods: Vec::new(),
        },
        // What a `Canvas`'s block draws with: its methods are in `method_of`.
        StructDef {
            name: "Graphics".into(),
            span: Span::default(),
            fields: vec![field("width", Ty::Float), field("height", Ty::Float)],
            methods: Vec::new(),
        },
        // What `.onPointer` tells its block (the runtime's `PointerC`).
        StructDef {
            name: "PointerEvent".into(),
            span: Span::default(),
            fields: vec![
                field("kind", POINTER_KIND),
                field("x", Ty::Float),
                field("y", Ty::Float),
                field("dx", Ty::Float),
                field("dy", Ty::Float),
                field("button", POINTER_BUTTON),
                field("clicks", Ty::Int),
                field("shift", Ty::Bool),
                field("alt", Ty::Bool),
                field("command", Ty::Bool),
            ],
            methods: Vec::new(),
        },
    ]
}

/// The `HttpResponse` struct (see `structs`).
pub const HTTP_RESPONSE: Ty = Ty::Struct(0);
/// The `Completion` struct (see `structs`).
pub const COMPLETION: Ty = Ty::Struct(1);
/// The `Graphics` struct (see `structs`).
pub const GRAPHICS: Ty = Ty::Struct(2);
/// The `PointerEvent` struct (see `structs`).
pub const POINTER_EVENT: Ty = Ty::Struct(3);

/// The built-in enums, by their place in `enums`.
pub const COLOR: Ty = Ty::Enum(0);
pub const WEIGHT: Ty = Ty::Enum(1);
pub const ALIGNMENT: Ty = Ty::Enum(2);
pub const POINTER_KIND: Ty = Ty::Enum(5);
pub const POINTER_BUTTON: Ty = Ty::Enum(6);
pub const CURSOR_STYLE: Ty = Ty::Enum(7);

pub struct Builtins {
    pub color: Ty,
    pub weight: Ty,
    pub alignment: Ty,
    pub icon: Ty,
    pub appearance: Ty,
}

impl Builtins {
    /// The built-in enums are always the first ones (see `enums`).
    pub fn standard() -> Builtins {
        Builtins { color: Ty::Enum(0), weight: Ty::Enum(1), alignment: Ty::Enum(2), icon: Ty::Enum(3), appearance: Ty::Enum(4) }
    }
}

pub const VIEW_NAMES: &[&str] =
    &[
        "Text", "Button", "TextField", "Toggle", "Picker", "VStack", "HStack", "ZStack", "Spacer", "Scroll", "Image",
        "CodeEditor", "TabView", "Divider", "Icon", "Svg", "Markdown", "Canvas",
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
                P::new("completions", Ty::list(COMPLETION)).default(),
                P::new("completionStart", Ty::Int).default(),
                P::new("onComplete", Ty::func(vec![Ty::Int], Ty::Void)).default(),
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
            "Icon" => vec![P::unlabeled("name", self.icon.clone())],
            "Svg" => vec![P::unlabeled("source", Ty::Str)],
            "Markdown" => vec![
                P::unlabeled("text", Ty::Str),
                P::new("folder", Ty::Str).default(),
                P::new("onLink", Ty::opt(Ty::func(vec![Ty::Str], Ty::Void))).default(),
            ],
            "VStack" | "HStack" => vec![
                P::new("spacing", Ty::Float).default(),
                P::new("alignment", self.alignment.clone()).default(),
                content(),
            ],
            "ZStack" => vec![P::new("alignment", self.alignment.clone()).default(), content()],
            "Scroll" => vec![content()],
            "Spacer" => vec![],
            "Image" => vec![P::unlabeled("path", Ty::Str)],
            "Canvas" => vec![P::new("redraw", Ty::Int).default(), P::new("draw", Ty::func(vec![GRAPHICS], Ty::Void))],
            _ => return None,
        })
    }

    /// Modifiers callable on any `View`. `id` and `tag` are checked specially.
    pub fn modifier(&self, name: &str) -> Option<Vec<P>> {
        Some(match name {
            "padding" => {
                let edge = |l: &str| P::new(l, Ty::Float).default();
                vec![
                    P::unlabeled("amount", Ty::Float).default(),
                    edge("horizontal"),
                    edge("vertical"),
                    edge("top"),
                    edge("leading"),
                    edge("bottom"),
                    edge("trailing"),
                ]
            }
            "font" => vec![P::new("size", Ty::Float).default(), P::new("weight", self.weight.clone()).default()],
            "color" => vec![P::unlabeled("color", self.color.clone())],
            "background" => vec![P::unlabeled("color", self.color.clone()), P::new("radius", Ty::Float).default()],
            "border" => vec![
                P::unlabeled("color", self.color.clone()),
                P::new("width", Ty::Float).default(),
                P::new("radius", Ty::Float).default(),
            ],
            "frame" => {
                vec![
                    P::new("width", Ty::opt(Ty::Float)).default(),
                    P::new("height", Ty::opt(Ty::Float)).default(),
                    P::new("minWidth", Ty::opt(Ty::Float)).default(),
                    P::new("maxWidth", Ty::opt(Ty::Float)).default(),
                    P::new("minHeight", Ty::opt(Ty::Float)).default(),
                    P::new("maxHeight", Ty::opt(Ty::Float)).default(),
                    P::new("alignment", self.alignment.clone()).default(),
                ]
            }
            "opacity" => vec![P::unlabeled("value", Ty::Float)],
            "hidden" => vec![P::unlabeled("isHidden", Ty::Bool).default()],
            "disabled" => vec![P::unlabeled("isDisabled", Ty::Bool).default()],
            "onTap" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "onDrag" => vec![P::unlabeled("action", Ty::func(vec![Ty::Float, Ty::Float], Ty::Void))],
            "onPointer" => vec![P::unlabeled("action", Ty::func(vec![POINTER_EVENT], Ty::Void))],
            "onKey" => vec![P::unlabeled("action", Ty::func(vec![Ty::Str], Ty::Void))],
            "cursor" => vec![P::unlabeled("style", CURSOR_STYLE)],
            "offset" => vec![P::new("x", Ty::Float).default(), P::new("y", Ty::Float).default()],
            "clipped" => vec![P::unlabeled("isClipped", Ty::Bool).default()],
            "onDrop" => vec![P::unlabeled("action", Ty::func(vec![Ty::Str], Ty::Void))],
            "tooltip" => vec![P::unlabeled("text", Ty::Str)],
            "onResize" => vec![P::unlabeled("action", Ty::func(vec![Ty::Float, Ty::Float], Ty::Void))],
            "id" | "tag" => vec![P::unlabeled("value", Ty::Any)],
            "shortcut" => vec![P::unlabeled("key", Ty::Str), P::new("action", Ty::func(vec![], Ty::Void))],
            "onAppear" | "onSubmit" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "tabItem" => vec![P::unlabeled("title", Ty::Str)],
            "autofocus" => vec![],
            "animation" => vec![P::unlabeled("seconds", Ty::Float).default()],
            "textAlignment" => vec![P::unlabeled("alignment", self.alignment.clone())],
            "hoverBackground" => vec![P::unlabeled("color", self.color.clone()), P::new("radius", Ty::Float).default()],
            _ => return None,
        })
    }

    pub fn function(&self, name: &str) -> Option<FnSig> {
        Some(match name {
            "print" | "printError" => FnSig {
                params: vec![P::unlabeled("value", Ty::Any), P::new("terminator", Ty::Str).default()],
                ret: Ty::Void,
            },
            "readLine" => FnSig { params: vec![], ret: Ty::opt(Ty::Str) },
            "readInput" => FnSig { params: vec![], ret: Ty::Str },
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
            "sqrt" | "exp" | "log" | "log10" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "floor"
            | "ceil" | "round" => FnSig { params: vec![P::unlabeled("x", Ty::Float)], ret: Ty::Float },
            "pow" => FnSig { params: vec![P::unlabeled("x", Ty::Float), P::unlabeled("power", Ty::Float)], ret: Ty::Float },
            "atan2" => FnSig { params: vec![P::unlabeled("y", Ty::Float), P::unlabeled("x", Ty::Float)], ret: Ty::Float },
            // `abs`, `min` and `max` take Int or Float: checked specially.
            "abs" => FnSig { params: vec![P::unlabeled("x", Ty::Any)], ret: Ty::Any },
            "min" | "max" => FnSig { params: vec![P::unlabeled("a", Ty::Any), P::unlabeled("b", Ty::Any)], ret: Ty::Any },
            "now" | "randomFloat" => FnSig { params: vec![], ret: Ty::Float },
            "isDarkMode" => FnSig { params: vec![], ret: Ty::Bool },
            "formatDate" => FnSig {
                params: vec![
                    P::unlabeled("time", Ty::Float),
                    P::new("format", Ty::Str).default(),
                    P::new("timeZone", Ty::Str).default(),
                ],
                ret: Ty::Str,
            },
            "parseDate" => FnSig {
                params: vec![
                    P::unlabeled("text", Ty::Str),
                    P::new("format", Ty::Str).default(),
                    P::new("timeZone", Ty::Str).default(),
                ],
                ret: Ty::opt(Ty::Float),
            },
            "addToDate" => FnSig {
                params: vec![
                    P::unlabeled("time", Ty::Float),
                    P::new("years", Ty::Int).default(),
                    P::new("months", Ty::Int).default(),
                    P::new("days", Ty::Int).default(),
                    P::new("hours", Ty::Int).default(),
                    P::new("minutes", Ty::Int).default(),
                    P::new("seconds", Ty::Int).default(),
                    P::new("timeZone", Ty::Str).default(),
                ],
                ret: Ty::Float,
            },
            "daysBetween" => FnSig {
                params: vec![P::unlabeled("from", Ty::Float), P::unlabeled("to", Ty::Float), P::new("timeZone", Ty::Str).default()],
                ret: Ty::Int,
            },
            "timeZoneOffset" => FnSig {
                params: vec![P::unlabeled("time", Ty::Float), P::new("timeZone", Ty::Str).default()],
                ret: Ty::Int,
            },
            "localTimeZone" => FnSig { params: vec![], ret: Ty::Str },
            "timeZones" => FnSig { params: vec![], ret: Ty::list(Ty::Str) },
            // For the prelude's `dateParts`.
            "__dateParts" => FnSig {
                params: vec![P::unlabeled("time", Ty::Float), P::unlabeled("timeZone", Ty::Str)],
                ret: Ty::list(Ty::Int),
            },
            "date" => FnSig {
                params: vec![
                    P::new("year", Ty::Int),
                    P::new("month", Ty::Int),
                    P::new("day", Ty::Int),
                    P::new("hour", Ty::Int).default(),
                    P::new("minute", Ty::Int).default(),
                    P::new("second", Ty::Int).default(),
                    P::new("timeZone", Ty::Str).default(),
                ],
                ret: Ty::Float,
            },
            "every" => FnSig {
                params: vec![P::new("seconds", Ty::Float), P::new("action", Ty::func(vec![], Ty::Void))],
                ret: Ty::Int,
            },
            "stopTimer" => FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: Ty::Void },
            "random" => FnSig { params: vec![P::unlabeled("range", Ty::Range)], ret: Ty::Int },
            "appendFile" => FnSig { params: vec![P::new("path", Ty::Str), P::new("text", Ty::Str)], ret: Ty::Bool },
            "copyFile" => FnSig { params: vec![P::new("from", Ty::Str), P::new("to", Ty::Str)], ret: Ty::Bool },
            "fileSize" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::opt(Ty::Int) },
            "modifiedTime" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::opt(Ty::Float) },
            "homeFolder" | "documentsFolder" | "tempFolder" | "platform" => FnSig { params: vec![], ret: Ty::Str },
            "appDataFolder" => FnSig { params: vec![P::unlabeled("name", Ty::Str)], ret: Ty::Str },
            "joinPath" => FnSig { params: vec![P::unlabeled("folder", Ty::Str), P::unlabeled("name", Ty::Str)], ret: Ty::Str },
            "arguments" => FnSig { params: vec![], ret: Ty::list(Ty::Str) },
            "exit" => FnSig { params: vec![P::unlabeled("code", Ty::Int).default()], ret: Ty::Void },
            "openURL" => FnSig { params: vec![P::unlabeled("url", Ty::Str)], ret: Ty::Bool },
            "copyToClipboard" => FnSig { params: vec![P::unlabeled("text", Ty::Str)], ret: Ty::Bool },
            "clipboardText" => FnSig { params: vec![], ret: Ty::opt(Ty::Str) },
            // For the prelude (`readText` and the others that return a `Result`).
            "__lastError" => FnSig { params: vec![], ret: Ty::Str },
            "__clearError" => FnSig { params: vec![], ret: Ty::Void },
            "decodeBase64" => FnSig { params: vec![P::unlabeled("text", Ty::Str)], ret: Ty::opt(Ty::Data) },
            "request" => FnSig {
                params: vec![
                    P::new("url", Ty::Str),
                    P::new("method", Ty::Str).default(),
                    P::new("headers", Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Str))).default(),
                    P::new("body", Ty::Str).default(),
                    P::new("done", Ty::func(vec![HTTP_RESPONSE], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "cancelRequest" => FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: Ty::Void },
            "download" => FnSig {
                params: vec![
                    P::new("url", Ty::Str),
                    P::new("to", Ty::Str),
                    P::new("progress", Ty::opt(Ty::func(vec![Ty::Float], Ty::Void))).default(),
                    P::new("done", Ty::func(vec![Ty::Bool], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "openWebSocket" => FnSig {
                params: vec![
                    P::new("url", Ty::Str),
                    P::new("onMessage", Ty::func(vec![Ty::Str], Ty::Void)),
                    P::new("onData", Ty::opt(Ty::func(vec![Ty::Data], Ty::Void))).default(),
                    P::new("onClose", Ty::func(vec![Ty::Str], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "sendWebSocket" => FnSig { params: vec![P::unlabeled("id", Ty::Int), P::new("text", Ty::Str)], ret: Ty::Bool },
            "sendWebSocketData" => FnSig { params: vec![P::unlabeled("id", Ty::Int), P::new("data", Ty::Data)], ret: Ty::Bool },
            "closeWebSocket" => FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: Ty::Void },
            "readData" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::opt(Ty::Data) },
            "writeImage" => FnSig {
                params: vec![
                    P::new("path", Ty::Str),
                    P::new("width", Ty::Int),
                    P::new("height", Ty::Int),
                    P::new("pixels", Ty::list(Ty::Int)),
                ],
                ret: Ty::Bool,
            },
            "renderImage" => FnSig {
                params: vec![
                    P::new("path", Ty::Str),
                    P::new("width", Ty::Float),
                    P::new("height", Ty::Float),
                    P::new("scale", Ty::Float).default(),
                    P::new("draw", Ty::func(vec![GRAPHICS], Ty::Void)),
                ],
                ret: Ty::Bool,
            },
            // For the prelude's `imageSize`.
            "__imageSize" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::list(Ty::Int) },
            // For the prelude's `readImage`.
            "__imagePixels" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::list(Ty::Int) },
            "setWindowTitle" => FnSig { params: vec![P::unlabeled("title", Ty::Str)], ret: Ty::Void },
            "writeData" => FnSig { params: vec![P::new("path", Ty::Str), P::new("data", Ty::Data)], ret: Ty::Bool },
            "appendData" => FnSig { params: vec![P::new("path", Ty::Str), P::new("data", Ty::Data)], ret: Ty::Bool },
            "openFile" => FnSig {
                params: vec![P::new("title", Ty::Str).default(), P::new("types", Ty::list(Ty::Str)).default()],
                ret: Ty::opt(Ty::Str),
            },
            "saveFile" => FnSig {
                params: vec![
                    P::new("title", Ty::Str).default(),
                    P::new("name", Ty::Str).default(),
                    P::new("types", Ty::list(Ty::Str)).default(),
                ],
                ret: Ty::opt(Ty::Str),
            },
            _ => return None,
        })
    }
}

pub const FUNCTION_NAMES: &[&str] = &[
    "print", "fetch", "after", "Int", "Float", "String", "readFile", "writeFile", "listFolder", "fileExists",
    "isFolder", "deleteFile", "createFolder", "renameFile", "currentFolder", "environment", "runCommand",
    "stopCommand", "sqrt", "pow", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2",
    "floor", "ceil", "round", "abs", "min", "max", "openFile", "saveFile", "Data", "readData", "writeData", "appendData",
    "toJson", "fromJson", "toBinary", "fromBinary", "now", "randomFloat", "formatDate", "parseDate", "date",
    "every", "stopTimer", "random", "appendFile", "copyFile", "fileSize", "modifiedTime", "homeFolder",
    "documentsFolder", "tempFolder", "platform", "appDataFolder", "joinPath", "arguments", "exit", "openURL",
    "copyToClipboard", "clipboardText", "decodeBase64", "request", "download", "openWebSocket", "sendWebSocket",
    "closeWebSocket", "background", "printError", "readLine", "readInput", "__lastError", "__clearError", "Set", "zip",
    "isDarkMode", "addToDate", "daysBetween", "timeZoneOffset", "localTimeZone", "timeZones", "__dateParts",
    "cancelRequest", "sendWebSocketData", "writeImage", "__imageSize", "__imagePixels", "setWindowTitle", "renderImage",
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
        (Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Set(_), "count") => Ty::Int,
        (Ty::List(_) | Ty::Str | Ty::Dict(..) | Ty::Set(_), "isEmpty") => Ty::Bool,
        (Ty::Set(t), "values") => Ty::list((**t).clone()),
        (Ty::List(t), "first" | "last") => Ty::opt((**t).clone()),
        (Ty::List(_), "indices") => Ty::Range,
        (Ty::Dict(k, _), "keys") => Ty::list((**k).clone()),
        (Ty::Dict(_, v), "values") => Ty::list((**v).clone()),
        (Ty::Str, "characters") => Ty::list(Ty::Str),
        (Ty::Str, "urlEncoded" | "fileName" | "fileExtension" | "folder") => Ty::Str,
        (Ty::Str, "urlDecoded") => Ty::opt(Ty::Str),
        (Ty::Int, "isEven" | "isOdd") => Ty::Bool,
        (Ty::Float, "isNaN" | "isFinite") => Ty::Bool,
        (Ty::Data, "base64") => Ty::Str,
        (Ty::Data, "count") => Ty::Int,
        (Ty::Data, "isEmpty") => Ty::Bool,
        (Ty::Data, "bytes") => Ty::list(Ty::Int),
        (Ty::Data, "text") => Ty::opt(Ty::Str),
        (Ty::Data, "latin1") => Ty::Str,
        _ => return None,
    })
}

/// Can two values of `t` be compared with `==`?
fn equatable(t: &Ty) -> bool {
    match t {
        Ty::Fn(..) | Ty::View | Ty::Any => false,
        Ty::List(t) | Ty::Opt(t) | Ty::Set(t) => equatable(t),
        Ty::Dict(k, v) => equatable(k) && equatable(v),
        _ => true,
    }
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
                "index" | "lastIndex" if equatable(&t) => method(vec![P::new("of", t.clone())], Ty::opt(Ty::Int)),
                "firstIndex" => method(vec![P::new("where", pred(&t))], Ty::opt(Ty::Int)),
                "count" => method(vec![P::new("where", pred(&t))], Ty::Int),
                "any" | "all" => method(vec![P::new("where", pred(&t))], Ty::Bool),
                "sum" if matches!(t, Ty::Int | Ty::Float) => method(vec![], t),
                "min" | "max" if matches!(t, Ty::Int | Ty::Float | Ty::Str) => method(vec![], Ty::opt(t)),
                "prefix" | "suffix" => method(vec![P::unlabeled("count", Ty::Int)], Ty::list(t)),
                "dropFirst" | "dropLast" => method(vec![P::unlabeled("count", Ty::Int).default()], Ty::list(t)),
                "slice" => method(vec![P::new("from", Ty::Int), P::new("to", Ty::Int)], Ty::list(t)),
                "unique" if equatable(&t) => method(vec![], Ty::list(t)),
                "shuffled" => method(vec![], Ty::list(t)),
                "randomElement" => method(vec![], Ty::opt(t)),
                "removeFirst" | "removeLast" => mutating(vec![], t),
                "appendAll" => mutating(vec![P::unlabeled("items", Ty::list(t.clone()))], Ty::Void),
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
            "findLast" => method(vec![P::unlabeled("text", Ty::Str)], Ty::opt(Ty::Int)),
            "reversed" | "trimStart" | "trimEnd" | "capitalized" => method(vec![], Ty::Str),
            "repeated" => method(vec![P::unlabeled("times", Ty::Int)], Ty::Str),
            "padStart" | "padEnd" => {
                method(vec![P::unlabeled("length", Ty::Int), P::new("with", Ty::Str).default()], Ty::Str)
            }
            "character" => method(vec![P::new("at", Ty::Int)], Ty::opt(Ty::Str)),
            "words" => method(vec![], Ty::list(Ty::Str)),
            "occurrences" => method(vec![P::new("of", Ty::Str)], Ty::Int),
            _ => return None,
        },
        Ty::Int => match name {
            "clamped" => method(vec![P::new("min", Ty::Int), P::new("max", Ty::Int)], Ty::Int),
            "bytes" => method(vec![P::new("count", Ty::Int), P::new("bigEndian", Ty::Bool).default()], Ty::list(Ty::Int)),
            _ => return None,
        },
        Ty::Float => match name {
            "formatted" => method(vec![P::new("decimals", Ty::Int)], Ty::Str),
            "clamped" => method(vec![P::new("min", Ty::Float), P::new("max", Ty::Float)], Ty::Float),
            "bytes" => method(
                vec![P::new("count", Ty::Int).default(), P::new("bigEndian", Ty::Bool).default()],
                Ty::list(Ty::Int),
            ),
            _ => return None,
        },
        Ty::Data => match name {
            "slice" => method(vec![P::new("from", Ty::Int), P::new("to", Ty::Int)], Ty::Data),
            "compressed" => method(vec![P::unlabeled("format", Ty::Str).default()], Ty::Data),
            "decompressed" => method(vec![P::unlabeled("format", Ty::Str).default()], Ty::opt(Ty::Data)),
            "int8" | "int16" | "uint16" | "int32" | "uint32" | "int64" => {
                method(vec![P::new("at", Ty::Int), P::new("bigEndian", Ty::Bool).default()], Ty::Int)
            }
            "float32" | "float64" => {
                method(vec![P::new("at", Ty::Int), P::new("bigEndian", Ty::Bool).default()], Ty::Float)
            }
            _ => return None,
        },
        t if *t == GRAPHICS => return graphics_method(name),
        Ty::Set(t) => {
            let (t, set) = ((**t).clone(), recv.clone());
            match name {
                "contains" => method(vec![P::unlabeled("item", t)], Ty::Bool),
                "insert" | "remove" => mutating(vec![P::unlabeled("item", t)], Ty::Void),
                "union" | "intersection" | "subtracting" => method(vec![P::unlabeled("other", set.clone())], set),
                "isSubset" | "isSuperset" => method(vec![P::new("of", set)], Ty::Bool),
                "isDisjoint" => method(vec![P::new("with", set)], Ty::Bool),
                "filter" => method(vec![P::unlabeled("keep", pred(&t))], set),
                "sorted" => method(vec![P::new("by", Ty::func(vec![t.clone(), t.clone()], Ty::Bool)).default()], Ty::list(t)),
                _ => return None,
            }
        }
        Ty::Dict(k, v) => match name {
            "removeValue" => mutating(vec![P::new("forKey", (**k).clone())], Ty::opt((**v).clone())),
            "contains" => method(vec![P::new("key", (**k).clone())], Ty::Bool),
            _ => return None,
        },
        _ => return None,
    })
}

/// Methods of `Graphics`, which a `Canvas`'s block draws with.
fn graphics_method(name: &str) -> Option<Method> {
    let f = |l: &str| P::new(l, Ty::Float);
    let u = |l: &str| P::unlabeled(l, Ty::Float);
    let color = || P::unlabeled("color", COLOR);
    let labeled_color = || P::new("color", COLOR);
    Some(match name {
        "beginPath" | "closePath" | "save" | "restore" | "resetTransform" => method(vec![], Ty::Void),
        "moveTo" | "lineTo" | "translate" => method(vec![u("x"), u("y")], Ty::Void),
        "quadTo" => method(vec![u("cx"), u("cy"), u("x"), u("y")], Ty::Void),
        "curveTo" => method(vec![u("c1x"), u("c1y"), u("c2x"), u("c2y"), u("x"), u("y")], Ty::Void),
        "lines" => method(vec![P::unlabeled("points", Ty::list(Ty::Float)), P::new("closed", Ty::Bool).default()], Ty::Void),
        "rect" => method(vec![f("x"), f("y"), f("width"), f("height"), f("radius").default()], Ty::Void),
        "circle" => method(vec![f("x"), f("y"), f("radius")], Ty::Void),
        "ellipse" => method(vec![f("x"), f("y"), f("radiusX"), f("radiusY")], Ty::Void),
        "arc" => method(vec![f("x"), f("y"), f("radius"), f("from"), f("to")], Ty::Void),
        "fill" => method(vec![color(), P::new("evenOdd", Ty::Bool).default()], Ty::Void),
        "stroke" => method(vec![color(), f("width").default(), P::new("dash", Ty::list(Ty::Float)).default()], Ty::Void),
        "line" => method(vec![u("x1"), u("y1"), u("x2"), u("y2"), labeled_color(), f("width").default()], Ty::Void),
        "fillRect" => method(vec![f("x"), f("y"), f("width"), f("height"), labeled_color(), f("radius").default()], Ty::Void),
        "strokeRect" => method(
            vec![f("x"), f("y"), f("width"), f("height"), labeled_color(), f("lineWidth").default(), f("radius").default()],
            Ty::Void,
        ),
        "fillCircle" => method(vec![f("x"), f("y"), f("radius"), labeled_color()], Ty::Void),
        "strokeCircle" => method(vec![f("x"), f("y"), f("radius"), labeled_color(), f("lineWidth").default()], Ty::Void),
        "text" => method(
            vec![
                P::unlabeled("text", Ty::Str),
                f("x"),
                f("y"),
                f("size").default(),
                labeled_color().default(),
                P::new("weight", WEIGHT).default(),
                P::new("align", ALIGNMENT).default(),
                f("angle").default(),
                P::new("halo", COLOR).default(),
                f("haloWidth").default(),
            ],
            Ty::Void,
        ),
        "textWidth" => {
            method(vec![P::unlabeled("text", Ty::Str), f("size").default(), P::new("weight", WEIGHT).default()], Ty::Float)
        }
        "image" => method(
            vec![P::unlabeled("path", Ty::Str), f("x"), f("y"), f("width"), f("height"), f("opacity").default()],
            Ty::Bool,
        ),
        "scale" => method(vec![u("x"), u("y")], Ty::Void),
        "rotate" => method(vec![u("angle")], Ty::Void),
        "zoom" => method(vec![u("factor")], Ty::Void),
        "clip" => method(vec![f("x"), f("y"), f("width"), f("height")], Ty::Void),
        _ => return None,
    })
}

/// Names shown in "did you mean" suggestions for members of built-in types.
pub fn member_names(recv: &Ty) -> &'static [&'static str] {
    match recv {
        Ty::List(_) => &[
            "count", "isEmpty", "first", "last", "append", "insert", "remove", "removeAll", "filter", "contains", "enumerated",
            "reversed", "map", "sorted", "joined", "index", "lastIndex", "firstIndex", "any", "all", "sum", "min",
            "max", "prefix", "suffix", "dropFirst", "dropLast", "slice", "unique", "shuffled", "randomElement",
            "removeFirst", "removeLast", "appendAll", "indices",
        ],
        Ty::Str => &[
            "count", "isEmpty", "contains", "hasPrefix", "hasSuffix", "split", "trim", "uppercase", "lowercase",
            "replace", "lines", "substring", "find", "findLast", "reversed", "trimStart", "trimEnd", "capitalized",
            "repeated", "padStart", "padEnd", "character", "words", "occurrences", "characters", "urlEncoded",
            "urlDecoded", "fileName", "fileExtension", "folder",
        ],
        Ty::Int => &["isEven", "isOdd", "clamped", "bytes"],
        Ty::Dict(..) => &["count", "isEmpty", "keys", "values", "removeValue", "contains"],
        Ty::Set(_) => &[
            "count", "isEmpty", "values", "contains", "insert", "remove", "union", "intersection", "subtracting",
            "isSubset", "isSuperset", "isDisjoint", "filter", "sorted",
        ],
        Ty::Float => &["formatted", "clamped", "isNaN", "isFinite", "bytes"],
        Ty::Data => &[
            "count", "isEmpty", "bytes", "text", "base64", "latin1", "slice", "int8", "int16", "uint16", "int32", "uint32",
            "int64", "float32", "float64", "compressed", "decompressed",
        ],
        t if *t == GRAPHICS => &[
            "width", "height", "beginPath", "moveTo", "lineTo", "quadTo", "curveTo", "closePath", "lines", "rect", "circle",
            "ellipse", "arc", "fill", "stroke", "line", "fillRect", "strokeRect", "fillCircle", "strokeCircle", "text",
            "textWidth", "image", "translate", "scale", "rotate", "zoom", "resetTransform", "save", "restore", "clip",
        ],
        Ty::View => &[
            "padding", "font", "color", "background", "border", "frame", "opacity", "hidden", "disabled", "onTap",
            "id", "tag", "shortcut", "onAppear", "onSubmit", "tabItem", "hoverBackground", "autofocus",
            "textAlignment", "animation", "onDrag", "onPointer", "onKey", "cursor", "offset", "clipped", "onDrop",
            "tooltip", "onResize",
        ],
        _ => &[],
    }
}

// ===== standard library functions that are plain runtime calls =====

/// A default argument value for a runtime call.
#[derive(Clone, Copy, Debug)]
pub enum Dflt {
    /// The argument is required.
    Req,
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(&'static str),
    EmptyList,
    /// A case (without values) of the parameter's enum, by its place.
    Case(i32),
}

/// A built-in function or method that compiles to a call of runtime function
/// `rt`, with the arguments in parameter order (a method's receiver first).
///
/// Types map to the C ABI as: Int → i64, Float → f64, Bool → u8, String,
/// Data, lists and dictionaries → pointers (borrowed). Results: an optional
/// String/Data/list/dictionary is a pointer, null for nil; an optional Int or
/// Float comes back through a trailing out pointer, with a u8 "present"
/// result. Owned results (strings, lists…) are new references.
///
/// An enum goes as its case's place (an i32), followed, for an enum whose
/// cases have values (`Color`), by the box of those values (a pointer).
#[derive(Clone, Copy, Debug)]
pub struct RtCall {
    pub rt: &'static str,
    pub defaults: &'static [Dflt],
    /// The call's place in the source goes after the arguments (a pointer),
    /// for a runtime error's message.
    pub located: bool,
    /// A method whose receiver isn't passed (the methods of `Graphics`).
    pub no_receiver: bool,
}

const fn rt(rt: &'static str) -> RtCall {
    RtCall { rt, defaults: &[], located: false, no_receiver: false }
}

const fn rt_d(rt: &'static str, defaults: &'static [Dflt]) -> RtCall {
    RtCall { rt, defaults, located: false, no_receiver: false }
}

/// A `Data` method that reads a number: `bigEndian:` is false unless given,
/// and bytes that aren't there stop the program.
const fn rt_number(rt: &'static str) -> RtCall {
    RtCall { rt, defaults: &[Dflt::Req, Dflt::Bool(false)], located: true, no_receiver: false }
}

/// A `Graphics` method.
const fn rt_g(rt: &'static str, defaults: &'static [Dflt]) -> RtCall {
    RtCall { rt, defaults, located: false, no_receiver: true }
}

/// Built-in functions that are runtime calls (their signatures are in
/// `Builtins::function`).
pub fn runtime_fn(name: &str) -> Option<RtCall> {
    Some(match name {
        "now" => rt("tsl_now"),
        "isDarkMode" => rt("tsl_is_dark_mode"),
        "randomFloat" => rt("tsl_random_float"),
        "formatDate" => rt_d("tsl_format_date", &[Dflt::Req, Dflt::Str("yyyy-MM-dd HH:mm"), Dflt::Str("")]),
        "parseDate" => rt_d("tsl_parse_date", &[Dflt::Req, Dflt::Str("yyyy-MM-dd"), Dflt::Str("")]),
        "date" => {
            rt_d("tsl_make_date", &[Dflt::Req, Dflt::Req, Dflt::Req, Dflt::Int(0), Dflt::Int(0), Dflt::Int(0), Dflt::Str("")])
        }
        "addToDate" => rt_d(
            "tsl_add_to_date",
            &[Dflt::Req, Dflt::Int(0), Dflt::Int(0), Dflt::Int(0), Dflt::Int(0), Dflt::Int(0), Dflt::Int(0), Dflt::Str("")],
        ),
        "daysBetween" => rt_d("tsl_days_between", &[Dflt::Req, Dflt::Req, Dflt::Str("")]),
        "timeZoneOffset" => rt_d("tsl_time_zone_offset", &[Dflt::Req, Dflt::Str("")]),
        "localTimeZone" => rt("tsl_local_time_zone"),
        "timeZones" => rt("tsl_time_zones"),
        "__dateParts" => rt("tsl_date_parts"),
        "cancelRequest" => rt("tsl_cancel_request"),
        "sendWebSocketData" => rt("tsl_send_web_socket_data"),
        "stopTimer" => rt("tsl_stop_timer"),
        "appendFile" => rt("tsl_append_file"),
        "copyFile" => rt("tsl_copy_file"),
        "fileSize" => rt("tsl_file_size"),
        "modifiedTime" => rt("tsl_modified_time"),
        "homeFolder" => rt("tsl_home_folder"),
        "documentsFolder" => rt("tsl_documents_folder"),
        "tempFolder" => rt("tsl_temp_folder"),
        "appDataFolder" => rt("tsl_app_data_folder"),
        "joinPath" => rt("tsl_join_path"),
        "platform" => rt("tsl_platform"),
        "arguments" => rt("tsl_arguments"),
        "exit" => rt_d("tsl_exit", &[Dflt::Int(0)]),
        "openURL" => rt("tsl_open_url"),
        "copyToClipboard" => rt("tsl_copy_to_clipboard"),
        "clipboardText" => rt("tsl_clipboard_text"),
        "__lastError" => rt("tsl_last_error"),
        "__clearError" => rt("tsl_clear_error"),
        "readLine" => rt("tsl_read_line"),
        "readInput" => rt("tsl_read_input"),
        "decodeBase64" => rt("tsl_decode_base64"),
        "sendWebSocket" => rt("tsl_send_web_socket"),
        "closeWebSocket" => rt("tsl_close_web_socket"),
        "readData" => rt("tsl_read_data"),
        "writeImage" => rt("tsl_write_image"),
        "__imageSize" => rt("tsl_image_size"),
        "__imagePixels" => rt("tsl_image_pixels"),
        "setWindowTitle" => rt("tsl_set_window_title"),
        "writeData" => rt("tsl_write_data"),
        "appendData" => rt("tsl_append_data"),
        _ => return None,
    })
}

/// Built-in methods that are runtime calls (signatures in `method_of`).
pub fn runtime_method(recv: &Ty, name: &str) -> Option<RtCall> {
    Some(match (recv, name) {
        (Ty::Data, "text") => rt("tsl_data_text"),
        (Ty::Str, "findLast") => rt("tsl_str_find_last"),
        (Ty::Str, "reversed") => rt("tsl_str_reversed"),
        (Ty::Str, "trimStart") => rt("tsl_str_trim_start"),
        (Ty::Str, "trimEnd") => rt("tsl_str_trim_end"),
        (Ty::Str, "capitalized") => rt("tsl_str_capitalized"),
        (Ty::Str, "repeated") => rt("tsl_str_repeated"),
        (Ty::Str, "padStart") => rt_d("tsl_str_pad_start", &[Dflt::Req, Dflt::Str(" ")]),
        (Ty::Str, "padEnd") => rt_d("tsl_str_pad_end", &[Dflt::Req, Dflt::Str(" ")]),
        (Ty::Str, "character") => rt("tsl_str_character_at"),
        (Ty::Str, "words") => rt("tsl_str_words"),
        (Ty::Str, "occurrences") => rt("tsl_str_occurrences"),
        (Ty::Int, "clamped") => rt("tsl_int_clamped"),
        (Ty::Set(_), "union") => rt("tsl_set_union"),
        (Ty::Set(_), "intersection") => rt("tsl_set_intersection"),
        (Ty::Set(_), "subtracting") => rt("tsl_set_subtracting"),
        (Ty::Set(_), "isSubset") => rt("tsl_set_is_subset"),
        (Ty::Set(_), "isSuperset") => rt("tsl_set_is_superset"),
        (Ty::Set(_), "isDisjoint") => rt("tsl_set_is_disjoint"),
        (Ty::Float, "clamped") => rt("tsl_float_clamped"),
        (Ty::Int, "bytes") => rt_d("tsl_int_bytes", &[Dflt::Req, Dflt::Bool(false)]),
        (Ty::Float, "bytes") => rt_d("tsl_float_bytes", &[Dflt::Int(8), Dflt::Bool(false)]),
        (Ty::Data, "slice") => rt("tsl_data_slice"),
        (Ty::Data, "compressed") => rt_d("tsl_data_compressed", &[Dflt::Str("zlib")]),
        (Ty::Data, "decompressed") => rt_d("tsl_data_decompressed", &[Dflt::Str("zlib")]),
        (Ty::Data, "int8") => rt_number("tsl_data_int8"),
        (Ty::Data, "int16") => rt_number("tsl_data_int16"),
        (Ty::Data, "uint16") => rt_number("tsl_data_uint16"),
        (Ty::Data, "int32") => rt_number("tsl_data_int32"),
        (Ty::Data, "uint32") => rt_number("tsl_data_uint32"),
        (Ty::Data, "int64") => rt_number("tsl_data_int64"),
        (Ty::Data, "float32") => rt_number("tsl_data_float32"),
        (Ty::Data, "float64") => rt_number("tsl_data_float64"),
        (t, _) if *t == GRAPHICS => return graphics_call(name),
        _ => return None,
    })
}

/// The runtime calls behind the methods of `Graphics` (see `graphics_method`).
fn graphics_call(name: &str) -> Option<RtCall> {
    use Dflt::{Bool, Case, EmptyList, Float, Req};
    // `Color.primary` and `.clear`, `Weight.regular`, `Alignment.topLeading`.
    const PRIMARY: Dflt = Case(0);
    const CLEAR: Dflt = Case(12);
    const REGULAR: Dflt = Case(0);
    const TOP_LEADING: Dflt = Case(5);
    Some(match name {
        "beginPath" => rt_g("tsl_canvas_begin_path", &[]),
        "closePath" => rt_g("tsl_canvas_close_path", &[]),
        "save" => rt_g("tsl_canvas_save", &[]),
        "restore" => rt_g("tsl_canvas_restore", &[]),
        "resetTransform" => rt_g("tsl_canvas_reset_transform", &[]),
        "moveTo" => rt_g("tsl_canvas_move_to", &[]),
        "lineTo" => rt_g("tsl_canvas_line_to", &[]),
        "translate" => rt_g("tsl_canvas_translate", &[]),
        "scale" => rt_g("tsl_canvas_scale", &[]),
        "rotate" => rt_g("tsl_canvas_rotate", &[]),
        "zoom" => rt_g("tsl_canvas_zoom", &[]),
        "quadTo" => rt_g("tsl_canvas_quad_to", &[]),
        "curveTo" => rt_g("tsl_canvas_curve_to", &[]),
        "lines" => rt_g("tsl_canvas_lines", &[Req, Bool(false)]),
        "rect" => rt_g("tsl_canvas_rect", &[Req, Req, Req, Req, Float(0.0)]),
        "circle" => rt_g("tsl_canvas_circle", &[]),
        "ellipse" => rt_g("tsl_canvas_ellipse", &[]),
        "arc" => rt_g("tsl_canvas_arc", &[]),
        "fill" => rt_g("tsl_canvas_fill", &[Req, Bool(false)]),
        "stroke" => rt_g("tsl_canvas_stroke", &[Req, Float(1.0), EmptyList]),
        "line" => rt_g("tsl_canvas_line", &[Req, Req, Req, Req, Req, Float(1.0)]),
        "fillRect" => rt_g("tsl_canvas_fill_rect", &[Req, Req, Req, Req, Req, Float(0.0)]),
        "strokeRect" => rt_g("tsl_canvas_stroke_rect", &[Req, Req, Req, Req, Req, Float(1.0), Float(0.0)]),
        "fillCircle" => rt_g("tsl_canvas_fill_circle", &[]),
        "strokeCircle" => rt_g("tsl_canvas_stroke_circle", &[Req, Req, Req, Req, Float(1.0)]),
        "text" => rt_g(
            "tsl_canvas_text",
            &[Req, Req, Req, Float(14.0), PRIMARY, REGULAR, TOP_LEADING, Float(0.0), CLEAR, Float(0.0)],
        ),
        "textWidth" => rt_g("tsl_canvas_text_width", &[Req, Float(14.0), REGULAR]),
        "image" => rt_g("tsl_canvas_image", &[Req, Req, Req, Req, Req, Float(1.0)]),
        "clip" => rt_g("tsl_canvas_clip", &[]),
        _ => return None,
    })
}

/// Built-in properties that are runtime calls (types in `property`).
pub fn runtime_property(recv: &Ty, name: &str) -> Option<RtCall> {
    Some(match (recv, name) {
        (Ty::Str, "characters") => rt("tsl_str_characters"),
        (Ty::Str, "urlEncoded") => rt("tsl_str_url_encoded"),
        (Ty::Str, "urlDecoded") => rt("tsl_str_url_decoded"),
        (Ty::Str, "fileName") => rt("tsl_path_file_name"),
        (Ty::Str, "fileExtension") => rt("tsl_path_extension"),
        (Ty::Str, "folder") => rt("tsl_path_folder"),
        (Ty::Int, "isEven") => rt("tsl_int_is_even"),
        (Ty::Int, "isOdd") => rt("tsl_int_is_odd"),
        (Ty::Float, "isNaN") => rt("tsl_float_is_nan"),
        (Ty::Float, "isFinite") => rt("tsl_float_is_finite"),
        (Ty::Data, "base64") => rt("tsl_data_base64"),
        (Ty::Data, "count") => rt("tsl_data_len"),
        (Ty::Data, "isEmpty") => rt("tsl_data_is_empty"),
        (Ty::Data, "bytes") => rt("tsl_data_ints"),
        (Ty::Data, "text") => rt("tsl_data_text"),
        (Ty::Data, "latin1") => rt("tsl_data_latin1"),
        _ => return None,
    })
}
