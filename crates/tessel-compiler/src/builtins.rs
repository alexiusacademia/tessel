//! Signatures of the built-in views, modifiers, functions and methods.

use crate::source::Span;
use crate::types::{CaseDef, EnumDef, FieldDef, FnSig, ParamSig as P, StructDef, Ty};

pub const COLORS: &[&str] = &[
    "primary", "secondary", "gray", "red", "orange", "yellow", "green", "blue", "purple", "pink", "white", "black",
    "clear", "lightGray", "sidebar", "highlight", "separator", "background",
];

/// How an app looks: like the system (the default), or always light or dark.
pub const APPEARANCES: &[&str] = &["system", "light", "dark"];

/// In this order (the runtime relies on the tags): the four first, then the others.
pub const WEIGHTS: &[&str] = &["regular", "medium", "semibold", "bold", "thin", "ultraLight", "light", "heavy", "black"];

/// In this order (the runtime relies on the tags): stacks use the first three
/// plus `top`/`bottom`; frames and ZStacks use all of them.
pub const ALIGNMENTS: &[&str] =
    &["leading", "center", "trailing", "top", "bottom", "topLeading", "topTrailing", "bottomLeading", "bottomTrailing"];

/// What a mouse did, for `.onPointer` (the runtime relies on the order).
pub const POINTER_KINDS: &[&str] = &["down", "up", "move", "drag", "scroll", "pinch", "leave"];

/// Mouse buttons (the runtime relies on the order).
pub const POINTER_BUTTONS: &[&str] = &["left", "right", "middle"];

/// Sides of a view, for `.popover(edge:)` (the runtime relies on the order).
pub const EDGES: &[&str] = &["top", "bottom", "leading", "trailing"];
/// `RibbonSize`: how big a ribbon's button is.
pub const RIBBON_SIZES: &[&str] = &["small", "large"];
/// How an animation moves (`.animation(curve:)`), in the runtime's order.
pub const CURVES: &[&str] = &["easeInOut", "easeIn", "easeOut", "linear", "spring", "bouncy"];
/// Which ways a `Scroll` scrolls (the runtime relies on the order).
pub const SCROLL_AXES_CASES: &[&str] = &["vertical", "horizontal", "both"];
/// Where `scrollTo` puts a view (the runtime relies on the order).
pub const SCROLL_ALIGN_CASES: &[&str] = &["nearest", "top", "center", "bottom"];
/// How a `Picker` shows its options (the runtime relies on the order).
pub const PICKER_STYLE_CASES: &[&str] = &["segmented", "radio", "menu"];

/// Pointers for `.cursor(…)` (the runtime relies on the order).
pub const CURSOR_STYLES: &[&str] = &[
    "arrow", "hand", "crosshair", "grab", "grabbing", "text", "move", "wait", "notAllowed", "resizeHorizontal",
    "resizeVertical", "zoomIn", "zoomOut",
];

/// How `g.beginLayer` blends a layer onto what's under it (the runtime
/// relies on the order).
pub const BLEND_MODES: &[&str] = &[
    "normal", "multiply", "screen", "overlay", "darken", "lighten", "colorDodge", "colorBurn", "hardLight", "softLight",
    "difference", "exclusion", "hue", "saturation", "color", "luminosity",
];

/// Names of the built-in icons, in order (shared with the runtime).
pub const ICON_NAMES: &str = include_str!("../../tessel-runtime/icons/names.txt");

/// Built-in enums, registered before user types: Color, Weight, Alignment,
/// IconName, Appearance, PointerKind, PointerButton, CursorStyle, BlendMode, Edge,
/// RibbonSize, Curve, ScrollAxes, ScrollAlign, PickerStyle.
pub fn enums() -> Vec<EnumDef> {
    let simple = |name: &str, cases: &[&str]| EnumDef {
        name: name.into(),
        span: None,
        cases: cases.iter().map(|c| CaseDef { name: (*c).into(), fields: Vec::new(), span: Span::default() }).collect(),
        methods: Vec::new(),
    };
    let icons: Vec<&str> = ICON_NAMES.lines().map(str::trim).filter(|n| !n.is_empty()).collect();
    // After its named cases (the runtime relies on the order), `Color` has
    // `.rgb(red, green, blue)` (each 0 to 255) and `.rgba(…, opacity)` (0 to 1).
    let mut color = simple("Color", COLORS);
    let channels = || vec![P::unlabeled("red", Ty::Int), P::unlabeled("green", Ty::Int), P::unlabeled("blue", Ty::Int)];
    color.cases.push(CaseDef { name: "rgb".into(), fields: channels(), span: Span::default() });
    let mut with_opacity = channels();
    with_opacity.push(P::unlabeled("opacity", Ty::Float));
    color.cases.push(CaseDef { name: "rgba".into(), fields: with_opacity, span: Span::default() });
    vec![
        color,
        simple("Weight", WEIGHTS),
        simple("Alignment", ALIGNMENTS),
        simple("IconName", &icons),
        simple("Appearance", APPEARANCES),
        simple("PointerKind", POINTER_KINDS),
        simple("PointerButton", POINTER_BUTTONS),
        simple("CursorStyle", CURSOR_STYLES),
        simple("BlendMode", BLEND_MODES),
        simple("Edge", EDGES),
        simple("RibbonSize", RIBBON_SIZES),
        simple("Curve", CURVES),
        simple("ScrollAxes", SCROLL_AXES_CASES),
        simple("ScrollAlign", SCROLL_ALIGN_CASES),
        simple("PickerStyle", PICKER_STYLE_CASES),
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
            // The URL that answered, after any redirects.
            field("finalUrl", Ty::Str),
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
                field("windowX", Ty::Float),
                field("windowY", Ty::Float),
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
        // What `.onKeyEvent` tells its block (the runtime's `KeyC`).
        StructDef {
            name: "KeyEvent".into(),
            span: Span::default(),
            fields: vec![
                field("key", Ty::Str),
                field("text", Ty::Str),
                field("shift", Ty::Bool),
                field("alt", Ty::Bool),
                field("command", Ty::Bool),
                field("control", Ty::Bool),
                field("repeated", Ty::Bool),
                // Let go (with `repeated` false); otherwise pressed.
                field("up", Ty::Bool),
            ],
            methods: Vec::new(),
        },
        // Work started with `background`: its methods are in `method_of`.
        StructDef {
            name: "BackgroundJob".into(),
            span: Span::default(),
            fields: vec![field("id", Ty::Int)],
            methods: Vec::new(),
        },
        // What a `Scene3D`'s block fills: its methods are in `method_of`.
        StructDef {
            name: "Scene".into(),
            span: Span::default(),
            fields: vec![field("width", Ty::Float), field("height", Ty::Float)],
            methods: Vec::new(),
        },
        // A sound playing (`playSound`): its methods are in `method_of`.
        StructDef {
            name: "Sound".into(),
            span: Span::default(),
            fields: vec![field("id", Ty::Int)],
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
/// The `KeyEvent` struct (see `structs`).
pub const KEY_EVENT: Ty = Ty::Struct(4);
/// The `BackgroundJob` struct (see `structs`).
pub const JOB: Ty = Ty::Struct(5);
/// The `Scene` struct (see `structs`).
pub const SCENE: Ty = Ty::Struct(6);
/// The `Sound` struct (see `structs`).
pub const SOUND: Ty = Ty::Struct(7);

/// The built-in enums, by their place in `enums`.
pub const COLOR: Ty = Ty::Enum(0);
pub const WEIGHT: Ty = Ty::Enum(1);
pub const ALIGNMENT: Ty = Ty::Enum(2);
pub const POINTER_KIND: Ty = Ty::Enum(5);
pub const POINTER_BUTTON: Ty = Ty::Enum(6);
pub const CURSOR_STYLE: Ty = Ty::Enum(7);
pub const BLEND_MODE: Ty = Ty::Enum(8);
pub const EDGE: Ty = Ty::Enum(9);
pub const RIBBON_SIZE: Ty = Ty::Enum(10);
pub const CURVE: Ty = Ty::Enum(11);
pub const SCROLL_AXES: Ty = Ty::Enum(12);
pub const SCROLL_ALIGN: Ty = Ty::Enum(13);
pub const PICKER_STYLE: Ty = Ty::Enum(14);

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
        "CodeEditor", "TabView", "Divider", "Icon", "Svg", "Markdown", "Canvas", "Scene3D", "Slider", "Stepper", "ColorPicker",
        "DisclosureGroup", "LazyVStack", "MenuItem", "MenuDivider", "Menu", "Window", "Ribbon", "RibbonTab", "RibbonGroup",
        "RibbonButton", "RibbonToggle", "RibbonMenu", "Table", "TableColumn", "Tree", "HSplit", "VSplit",
        "Grid", "GridRow", "LazyHStack", "SearchField", "__RichEditor", "VideoPlayer", "WebView",
    ];

/// Built-in views whose names a program may take for its own types (a
/// program's `struct Tree` was there first): the program's own is meant
/// then, and the view isn't to be had in it.
pub const YIELDING_VIEWS: &[&str] = &["Table", "TableColumn", "Tree", "Grid", "GridRow"];

/// Views that only go in menus (`.contextMenu`, `.menuBar`, `Menu`).
pub const MENU_VIEWS: &[&str] = &["MenuItem", "MenuDivider", "Menu"];

impl Builtins {
    pub fn view(&self, name: &str) -> Option<Vec<P>> {
        let content = || P::new("content", Ty::View);
        Some(match name {
            "Text" => vec![P::unlabeled("text", Ty::Str)],
            "Button" => vec![P::unlabeled("label", Ty::Str), P::new("action", Ty::func(vec![], Ty::Void))],
            "TextField" => vec![
                P::unlabeled("placeholder", Ty::Str),
                P::new("text", Ty::Str).bind(),
                P::new("secure", Ty::Bool).default(),
                P::new("lines", Ty::Int).default(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            "Toggle" => vec![P::unlabeled("label", Ty::Str).default(), P::new("isOn", Ty::Bool).bind(), P::new("onChange", Ty::func(vec![], Ty::Void)).default()],
            // The text of the prelude's `RichTextEditor` (see the runtime's `rich`).
            "__RichEditor" => vec![
                P::new("id", Ty::Int),
                P::new("json", Ty::Str),
                P::new("onChange", Ty::func(vec![Ty::Str], Ty::Void)),
            ],
            "SearchField" => vec![
                P::unlabeled("placeholder", Ty::Str).default(),
                P::new("text", Ty::Str).bind(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            "WebView" => vec![P::new("url", Ty::Str).default(), P::new("html", Ty::Str).default()],
            "VideoPlayer" => vec![
                P::unlabeled("path", Ty::Str),
                P::new("playing", Ty::Bool).bind().default(),
                P::new("position", Ty::Float).bind().default(),
                P::new("controls", Ty::Bool).default(),
                P::new("loop", Ty::Bool).default(),
                P::new("volume", Ty::Float).default(),
            ],
            "CodeEditor" => vec![
                P::new("text", Ty::Str).bind(),
                P::new("errorLines", Ty::list(Ty::Int)).default(),
                P::new("line", Ty::Int).bind().default(),
                P::new("completions", Ty::list(COMPLETION)).default(),
                P::new("completionStart", Ty::Int).default(),
                P::new("onComplete", Ty::func(vec![Ty::Int], Ty::Void)).default(),
                P::new("breakpoints", Ty::list(Ty::Int)).default(),
                P::new("stoppedLine", Ty::Int).default(),
                P::new("onBreakpoint", Ty::func(vec![Ty::Int], Ty::Void)).default(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            "Picker" => {
                let mut selection = P::new("selection", Ty::Any).bind();
                selection.sets_tag = true;
                vec![selection, P::new("style", PICKER_STYLE).default(), P::new("onChange", Ty::func(vec![], Ty::Void)).default(), content()]
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
            // Rows (`GridRow`s) of cells in columns that line up.
            "Grid" => vec![
                P::new("alignment", self.alignment.clone()).default(),
                P::new("spacing", Ty::Float).default(),
                P::new("rowSpacing", Ty::Float).default(),
                content(),
            ],
            "GridRow" => vec![content()],
            "Scroll" => vec![P::unlabeled("axes", SCROLL_AXES).default(), content()],
            // The first pane's width (height), when the app keeps it.
            "HSplit" | "VSplit" => vec![P::new("size", Ty::Float).bind().default(), content()],
            "Spacer" => vec![],
            "Image" => vec![P::unlabeled("path", Ty::Str)],
            "Canvas" => vec![P::new("redraw", Ty::Int).default(), P::new("draw", Ty::func(vec![GRAPHICS], Ty::Void))],
            "Scene3D" => vec![P::new("redraw", Ty::Int).default(), P::new("draw", Ty::func(vec![SCENE], Ty::Void))],
            "Slider" => vec![
                P::new("value", Ty::Float).bind(),
                P::new("from", Ty::Float).default(),
                P::new("to", Ty::Float).default(),
                P::new("step", Ty::Float).default(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            // `value:` is an `Int` or a `Float` (checked specially).
            "Stepper" => vec![
                P::new("value", Ty::Any).bind(),
                P::new("step", Ty::Float).default(),
                P::new("from", Ty::Float).default(),
                P::new("to", Ty::Float).default(),
                P::new("decimals", Ty::Int).default(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            "ColorPicker" => vec![P::new("color", self.color.clone()).bind(), P::new("onChange", Ty::func(vec![], Ty::Void)).default()],
            "DisclosureGroup" => vec![P::unlabeled("title", Ty::Str), P::new("isExpanded", Ty::Bool).bind(), content()],
            // The content block gets each row's number: `{ i in … }`.
            // Rows in columns: the block adds the `TableColumn`s, each of
            // which builds its cell in row `i` (see the runtime's `table`).
            "Table" => vec![
                P::new("rows", Ty::Int),
                P::new("rowHeight", Ty::Float).default(),
                P::new("selection", Ty::list(Ty::Int)).bind().default(),
                P::new("sortColumn", Ty::Int).bind().default(),
                P::new("ascending", Ty::Bool).bind().default(),
                P::new("onSort", Ty::func(vec![], Ty::Void)).default(),
                P::new("multiple", Ty::Bool).default(),
                P::new("onOpen", Ty::func(vec![Ty::Int], Ty::Void)).default(),
                content(),
            ],
            "TableColumn" => {
                let mut cells = P::new("content", Ty::func(vec![Ty::Int], Ty::View));
                cells.rows = true;
                vec![
                    P::unlabeled("title", Ty::Str),
                    P::new("width", Ty::Float).default(),
                    P::new("alignment", self.alignment.clone()).default(),
                    // Its cells can be edited: a double click shows a text
                    // field, and what's typed comes here (with the row).
                    P::new("onEdit", Ty::func(vec![Ty::Int, Ty::Str], Ty::Void)).default(),
                    cells,
                ]
            }
            // The program's tree: how many children a node has (by its path,
            // `[]` for the top), and the row of each that shows.
            "Tree" => {
                let path = Ty::list(Ty::Int);
                let mut rows = P::new("content", Ty::func(vec![path.clone()], Ty::View));
                rows.rows = true;
                vec![
                    P::new("children", Ty::func(vec![path.clone()], Ty::Int)),
                    P::new("rowHeight", Ty::Float).default(),
                    P::new("selection", path.clone()).bind().default(),
                    P::new("onOpen", Ty::func(vec![path], Ty::Void)).default(),
                    rows,
                ]
            }
            "LazyVStack" => {
                let mut rows = P::new("content", Ty::func(vec![Ty::Int], Ty::View));
                rows.rows = true;
                vec![P::new("count", Ty::Int), P::new("rowHeight", Ty::Float), rows]
            }
            "LazyHStack" => {
                let mut columns = P::new("content", Ty::func(vec![Ty::Int], Ty::View));
                columns.rows = true;
                vec![P::new("count", Ty::Int), P::new("columnWidth", Ty::Float), columns]
            }
            "MenuItem" => vec![
                P::unlabeled("title", Ty::Str),
                P::new("shortcut", Ty::Str).default(),
                P::new("checked", Ty::Bool).default(),
                P::new("action", Ty::func(vec![], Ty::Void)),
            ],
            "MenuDivider" => vec![],
            "Menu" => vec![P::unlabeled("title", Ty::Str), content()],
            "Ribbon" => vec![P::new("selection", Ty::Str).bind(), content()],
            "RibbonTab" => vec![P::unlabeled("title", Ty::Str), content()],
            // (`icon:` shows when the group is collapsed into one button.)
            "RibbonGroup" => vec![P::unlabeled("title", Ty::Str), P::new("icon", self.icon.clone()).default(), content()],
            "RibbonButton" => vec![
                P::unlabeled("label", Ty::Str),
                P::new("icon", self.icon.clone()),
                P::new("size", RIBBON_SIZE).default(),
                P::new("action", Ty::func(vec![], Ty::Void)),
            ],
            "RibbonToggle" => vec![
                P::unlabeled("label", Ty::Str),
                P::new("icon", self.icon.clone()),
                P::new("size", RIBBON_SIZE).default(),
                P::new("isOn", Ty::Bool).bind(),
                P::new("onChange", Ty::func(vec![], Ty::Void)).default(),
            ],
            "RibbonMenu" => vec![
                P::unlabeled("label", Ty::Str),
                P::new("icon", self.icon.clone()),
                P::new("size", RIBBON_SIZE).default(),
                content(),
            ],
            "Window" => vec![
                P::unlabeled("title", Ty::Str),
                P::new("isOpen", Ty::Bool).bind(),
                P::new("width", Ty::Float).default(),
                P::new("height", Ty::Float).default(),
                content(),
            ],
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
            "font" => vec![
                P::new("size", Ty::Float).default(),
                P::new("weight", self.weight.clone()).default(),
                P::new("italic", Ty::Bool).default(),
                P::new("family", Ty::Str).default(),
            ],
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
            "onKeyEvent" => vec![P::new("releases", Ty::Bool).default(), P::unlabeled("action", Ty::func(vec![KEY_EVENT], Ty::Void))],
            "cursor" => vec![P::unlabeled("style", CURSOR_STYLE)],
            "offset" => vec![P::new("x", Ty::Float).default(), P::new("y", Ty::Float).default()],
            "clipped" => vec![P::unlabeled("isClipped", Ty::Bool).default()],
            "onDrop" => vec![P::unlabeled("action", Ty::func(vec![Ty::Str], Ty::Void))],
            "draggable" => vec![P::unlabeled("payload", Ty::Str)],
            "dragFile" => vec![P::unlabeled("path", Ty::Str)],
            "dropTarget" => vec![
                P::new("isTargeted", Ty::Bool).bind().default(),
                P::unlabeled("action", Ty::func(vec![Ty::Str, Ty::Float, Ty::Float], Ty::Void)),
            ],
            "tooltip" | "accessibilityLabel" => vec![P::unlabeled("text", Ty::Str)],
            "onResize" => vec![P::unlabeled("action", Ty::func(vec![Ty::Float, Ty::Float], Ty::Void))],
            "contextMenu" | "menuBar" => vec![P::unlabeled("items", Ty::View)],
            "trayIcon" => vec![
                P::unlabeled("icon", self.icon.clone()),
                P::new("title", Ty::Str).default(),
                P::new("keepRunning", Ty::Bool).default(),
                P::unlabeled("items", Ty::View),
            ],
            "popover" => vec![
                P::new("isPresented", Ty::Bool).bind(),
                P::new("edge", EDGE).default(),
                P::unlabeled("content", Ty::View),
            ],
            "sheet" => vec![P::new("isPresented", Ty::Bool).bind(), P::unlabeled("content", Ty::View)],
            "onOpenFile" => vec![P::unlabeled("action", Ty::func(vec![Ty::Str], Ty::Void))],
            "onCloseRequest" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "id" | "tag" => vec![P::unlabeled("value", Ty::Any)],
            "shortcut" => vec![P::unlabeled("key", Ty::Str), P::new("action", Ty::func(vec![], Ty::Void))],
            "onAppear" | "onSubmit" => vec![P::unlabeled("action", Ty::func(vec![], Ty::Void))],
            "tabItem" => vec![P::unlabeled("title", Ty::Str)],
            "autofocus" => vec![],
            "animation" => vec![P::unlabeled("seconds", Ty::Float).default(), P::new("curve", CURVE).default()],
            "textAlignment" => vec![P::unlabeled("alignment", self.alignment.clone())],
            "hoverBackground" => vec![P::unlabeled("color", self.color.clone()), P::new("radius", Ty::Float).default()],
            "gridCellColumns" => vec![P::unlabeled("count", Ty::Int)],
            "gridColumnAlignment" => vec![P::unlabeled("alignment", self.alignment.clone())],
            "selectable" => vec![P::unlabeled("isSelectable", Ty::Bool).default()],
            "rotation" => vec![P::unlabeled("degrees", Ty::Float)],
            "scale" => vec![P::unlabeled("factor", Ty::Float).default(), P::new("x", Ty::Float).default(), P::new("y", Ty::Float).default()],
            "lineLimit" => vec![P::unlabeled("lines", Ty::Int)],
            _ => return None,
        })
    }

    pub fn function(&self, name: &str) -> Option<FnSig> {
        Some(match name {
            "print" | "printError" => FnSig {
                params: vec![
                    P::unlabeled("values", Ty::list(Ty::Any)).variadic(),
                    P::new("separator", Ty::Str).default(),
                    P::new("terminator", Ty::Str).default(),
                ],
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
                    P::new("input", Ty::Bool).default(),
                    P::new("onOutput", Ty::func(vec![Ty::Str], Ty::Void)),
                    P::new("onExit", Ty::func(vec![Ty::Int], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "stopCommand" => FnSig { params: vec![P::new("id", Ty::Int)], ret: Ty::Void },
            "sendToCommand" => FnSig { params: vec![P::unlabeled("id", Ty::Int), P::new("text", Ty::Str)], ret: Ty::Bool },
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
            "fontFamilies" => FnSig { params: vec![], ret: Ty::list(Ty::Str) },
            "chooseFolder" => FnSig { params: vec![P::new("title", Ty::Str).default()], ret: Ty::opt(Ty::Str) },
            "__openFiles" => {
                FnSig { params: vec![P::unlabeled("title", Ty::Str), P::unlabeled("types", Ty::list(Ty::Str))], ret: Ty::list(Ty::Str) }
            }
            "__printFile" => FnSig { params: vec![P::unlabeled("path", Ty::Str), P::unlabeled("title", Ty::Str)], ret: Ty::Bool },
            "__messageDialog" => FnSig {
                params: vec![
                    P::unlabeled("confirm", Ty::Bool),
                    P::unlabeled("title", Ty::Str),
                    P::unlabeled("message", Ty::Str),
                    P::unlabeled("confirmLabel", Ty::Str),
                    P::unlabeled("cancelLabel", Ty::Str),
                    P::unlabeled("level", Ty::Int),
                ],
                ret: Ty::Bool,
            },
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
            "homeFolder" | "documentsFolder" | "tempFolder" | "appPath" | "appFolder" | "platform" => FnSig { params: vec![], ret: Ty::Str },
            // (For the prelude's `RichTextEditor`: its id, the marks of what's
            // selected in it, and its toolbar's commands.)
            "__richId" => FnSig { params: vec![], ret: Ty::Int },
            "__richMarks" => FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: Ty::Int },
            "__richApply" => FnSig { params: vec![P::unlabeled("id", Ty::Int), P::unlabeled("command", Ty::Str)], ret: Ty::Void },
            "appDataFolder" => FnSig { params: vec![P::unlabeled("name", Ty::Str)], ret: Ty::Str },
            "joinPath" => FnSig { params: vec![P::unlabeled("folder", Ty::Str), P::unlabeled("name", Ty::Str)], ret: Ty::Str },
            "arguments" => FnSig { params: vec![], ret: Ty::list(Ty::Str) },
            "exit" => FnSig { params: vec![P::unlabeled("code", Ty::Int).default()], ret: Ty::Void },
            "openURL" => FnSig { params: vec![P::unlabeled("url", Ty::Str)], ret: Ty::Bool },
            // `id:` and `in:` are an `Int` or a `String` (checked specially).
            "scrollTo" => FnSig {
                params: vec![P::unlabeled("id", Ty::Any), P::new("in", Ty::Any).default(), P::new("align", SCROLL_ALIGN).default()],
                ret: Ty::Void,
            },
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
                    P::new("onHeaders", Ty::opt(Ty::func(vec![HTTP_RESPONSE], Ty::Void))).default(),
                    P::new("onData", Ty::opt(Ty::func(vec![Ty::Data], Ty::Void))).default(),
                    P::new("done", Ty::func(vec![HTTP_RESPONSE], Ty::Void)),
                ],
                ret: Ty::Int,
            },
            "cancelRequest" => FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: Ty::Void },
            "download" => FnSig {
                params: vec![
                    P::new("url", Ty::Str),
                    P::new("to", Ty::Str),
                    P::new("headers", Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Str))).default(),
                    P::new("onHeaders", Ty::opt(Ty::func(vec![HTTP_RESPONSE], Ty::Void))).default(),
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
            "writePDF" => FnSig {
                params: vec![
                    P::new("path", Ty::Str),
                    P::new("title", Ty::Str).default(),
                    P::new("width", Ty::Float).default(),
                    P::new("height", Ty::Float).default(),
                    P::new("draw", Ty::func(vec![GRAPHICS], Ty::Void)),
                ],
                ret: Ty::Bool,
            },
            // For the prelude's `imageSize`.
            "__imageSize" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::list(Ty::Int) },
            // Where the device is, its satellites and its compass: the
            // prelude makes values of the numbers (`location`, `satellites`,
            // `heading`, `requestPermission`).
            "__permission" => FnSig { params: vec![P::unlabeled("kind", Ty::Int), P::unlabeled("ask", Ty::Bool)], ret: Ty::Int },
            "startLocation" => FnSig {
                params: vec![P::new("precise", Ty::Bool).default(), P::new("interval", Ty::Float).default()],
                ret: Ty::Bool,
            },
            "startCompass" => FnSig { params: vec![], ret: Ty::Bool },
            "stopLocation" | "stopCompass" => FnSig { params: vec![], ret: Ty::Void },
            "__location" | "__satellites" | "__heading" => FnSig { params: vec![], ret: Ty::list(Ty::Float) },
            // For the prelude's `appInfo`.
            "__appInfo" => FnSig { params: vec![], ret: Ty::list(Ty::Str) },
            // Fonts: an outline as `g.text` draws the text (`textOutline`),
            // and for the prelude's `textGlyphs`, `fontMetrics` and
            // `fontStyles`, numbers it makes values of.
            "textOutline" | "__textGlyphs" => FnSig {
                params: vec![
                    P::unlabeled("text", Ty::Str),
                    P::new("size", Ty::Float),
                    P::new("weight", WEIGHT).default(),
                    P::new("italic", Ty::Bool).default(),
                    P::new("family", Ty::Str).default(),
                    P::new("kerning", Ty::Bool).default(),
                    P::new("tracking", Ty::Float).default(),
                ],
                ret: Ty::list(Ty::Float),
            },
            "__fontMetrics" => FnSig {
                params: vec![
                    P::new("size", Ty::Float),
                    P::new("weight", WEIGHT).default(),
                    P::new("italic", Ty::Bool).default(),
                    P::new("family", Ty::Str).default(),
                ],
                ret: Ty::list(Ty::Float),
            },
            "__fontStyleNames" => FnSig { params: vec![P::unlabeled("family", Ty::Str)], ret: Ty::list(Ty::Str) },
            "__fontStyleKinds" => FnSig { params: vec![P::unlabeled("family", Ty::Str)], ret: Ty::list(Ty::Float) },
            // For the prelude's `readImage`.
            "__imagePixels" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::list(Ty::Int) },
            // For the prelude's databases (see `Database`).
            "__dbOpen" => FnSig { params: vec![P::unlabeled("path", Ty::Str)], ret: Ty::Int },
            "__dbClose" | "__dbLastInsertId" => {
                FnSig { params: vec![P::unlabeled("id", Ty::Int)], ret: if name == "__dbClose" { Ty::Void } else { Ty::Int } }
            }
            "__dbExecute" | "__dbQuery" => FnSig {
                params: vec![P::unlabeled("id", Ty::Int), P::unlabeled("sql", Ty::Str), P::unlabeled("values", Ty::Str)],
                ret: if name == "__dbExecute" { Ty::Int } else { Ty::opt(Ty::Str) },
            },
            "__dbNoColumn" => FnSig {
                params: vec![P::unlabeled("column", Ty::Str), P::unlabeled("columns", Ty::list(Ty::Str))],
                ret: Ty::Int,
            },
            "setWindowTitle" => FnSig { params: vec![P::unlabeled("title", Ty::Str)], ret: Ty::Void },
            "setFullScreen" => FnSig { params: vec![P::unlabeled("on", Ty::Bool)], ret: Ty::Void },
            "showWindow" => FnSig { params: vec![], ret: Ty::Void },
            "__playSound" => FnSig {
                params: vec![P::unlabeled("path", Ty::Str), P::unlabeled("volume", Ty::Float), P::unlabeled("loop", Ty::Bool)],
                ret: Ty::Int,
            },
            "showNotification" => FnSig { params: vec![P::unlabeled("title", Ty::Str), P::new("message", Ty::Str).default()], ret: Ty::Void },
            "isFullScreen" => FnSig { params: vec![], ret: Ty::Bool },
            "undoTextEdit" | "redoTextEdit" => FnSig { params: vec![], ret: Ty::Void },
            "quit" | "closeWindow" | "dismiss" => FnSig { params: vec![], ret: Ty::Void },
            "showNotice" => FnSig { params: vec![P::unlabeled("text", Ty::Str), P::new("seconds", Ty::Float).default()], ret: Ty::Void },
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
    "stopCommand", "sendToCommand", "sqrt", "pow", "exp", "log", "log10", "sin", "cos", "tan", "asin", "acos", "atan", "atan2",
    "floor", "ceil", "round", "abs", "min", "max", "openFile", "saveFile", "Data", "readData", "writeData", "appendData",
    "toJson", "fromJson", "toBinary", "fromBinary", "now", "randomFloat", "formatDate", "parseDate", "date",
    "every", "stopTimer", "random", "appendFile", "copyFile", "fileSize", "modifiedTime", "homeFolder",
    "documentsFolder", "tempFolder", "appPath", "appFolder", "scrollTo", "__richId", "__richMarks", "__richApply", "platform", "appDataFolder", "joinPath", "arguments", "exit", "openURL",
    "copyToClipboard", "clipboardText", "decodeBase64", "request", "download", "openWebSocket", "sendWebSocket",
    "closeWebSocket", "background", "printError", "readLine", "readInput", "__lastError", "__clearError", "Set", "zip",
    "isDarkMode", "addToDate", "daysBetween", "timeZoneOffset", "localTimeZone", "timeZones", "fontFamilies", "chooseFolder", "__openFiles", "__messageDialog", "__printFile", "__dateParts",
    "cancelRequest", "sendWebSocketData", "writeImage", "__imageSize", "__imagePixels", "__appInfo", "__dbOpen", "__dbClose", "__dbLastInsertId", "__dbExecute", "__dbQuery",
    "__dbNoColumn", "setWindowTitle", "setFullScreen", "isFullScreen", "showWindow", "showNotification", "__playSound", "renderImage", "writePDF",
    "undoTextEdit", "redoTextEdit",
    "quit", "closeWindow", "dismiss", "showNotice",
    "textOutline", "__textGlyphs", "__fontMetrics", "__fontStyleNames", "__fontStyleKinds",
    "__permission", "startLocation", "stopLocation", "__location", "__satellites", "startCompass", "stopCompass", "__heading",
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
        (Ty::Data, "base64" | "hex") => Ty::Str,
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
            "wrappingAdd" | "wrappingSubtract" | "wrappingMultiply" => method(vec![P::unlabeled("other", Ty::Int)], Ty::Int),
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
            // (Also `append(byte: Int)`, which the checker handles.)
            "append" => mutating(vec![P::unlabeled("data", Ty::Data)], Ty::Void),
            "slice" => method(vec![P::new("from", Ty::Int), P::new("to", Ty::Int)], Ty::Data),
            "compressed" => method(vec![P::unlabeled("format", Ty::Str).default()], Ty::Data),
            "hash" => method(vec![P::unlabeled("algorithm", Ty::Str).default()], Ty::Data),
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
        t if *t == SCENE => return scene_method(name),
        t if *t == SOUND => match name {
            "pause" | "resume" | "stop" => method(vec![], Ty::Void),
            "seek" => method(vec![P::new("to", Ty::Float)], Ty::Void),
            "setVolume" => method(vec![P::unlabeled("volume", Ty::Float)], Ty::Void),
            "isPlaying" => method(vec![], Ty::Bool),
            "position" | "duration" => method(vec![], Ty::Float),
            _ => return None,
        },
        t if *t == JOB => match name {
            "progress" => method(vec![P::unlabeled("fraction", Ty::Float), P::new("note", Ty::Str).default()], Ty::Void),
            "isCancelled" => method(vec![], Ty::Bool),
            "cancel" => method(vec![], Ty::Void),
            _ => return None,
        },
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
        "newPage" => method(vec![f("width").default(), f("height").default()], Ty::Void),
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
                P::new("italic", Ty::Bool).default(),
                P::new("family", Ty::Str).default(),
                P::new("kerning", Ty::Bool).default(),
                f("tracking").default(),
            ],
            Ty::Void,
        ),
        "textPath" => method(
            vec![
                P::unlabeled("text", Ty::Str),
                f("x"),
                f("y"),
                f("size").default(),
                P::new("weight", WEIGHT).default(),
                P::new("align", ALIGNMENT).default(),
                P::new("italic", Ty::Bool).default(),
                P::new("family", Ty::Str).default(),
                P::new("kerning", Ty::Bool).default(),
                f("tracking").default(),
            ],
            Ty::Void,
        ),
        "textWidth" => method(
            vec![
                P::unlabeled("text", Ty::Str),
                f("size").default(),
                P::new("weight", WEIGHT).default(),
                P::new("italic", Ty::Bool).default(),
                P::new("family", Ty::Str).default(),
                P::new("kerning", Ty::Bool).default(),
                f("tracking").default(),
            ],
            Ty::Float,
        ),
        "image" => method(
            vec![P::unlabeled("path", Ty::Str), f("x"), f("y"), f("width"), f("height"), f("opacity").default()],
            Ty::Bool,
        ),
        "image(pixels:)" => method(
            vec![
                P::new("pixels", Ty::list(Ty::Int)),
                P::new("pixelWidth", Ty::Int),
                P::new("pixelHeight", Ty::Int),
                f("x"),
                f("y"),
                f("width"),
                f("height"),
                f("opacity").default(),
            ],
            Ty::Bool,
        ),
        "svg" | "svg(text:)" => {
            let source = if name == "svg" { P::unlabeled("path", Ty::Str) } else { P::new("text", Ty::Str) };
            method(
                vec![source, f("x"), f("y"), f("width"), f("height"), labeled_color().default(), f("opacity").default()],
                Ty::Bool,
            )
        }
        "fillLinear" | "strokeLinear" | "fillRadial" | "strokeRadial" => {
            let mut params = if name.ends_with("Linear") {
                vec![f("x1"), f("y1"), f("x2"), f("y2")]
            } else {
                vec![f("x"), f("y"), f("radius")]
            };
            params.push(P::new("colors", Ty::list(COLOR)));
            params.push(P::new("stops", Ty::list(Ty::Float)).default());
            if name.starts_with("fill") {
                params.push(P::new("evenOdd", Ty::Bool).default());
            } else {
                params.push(f("width").default());
                params.push(P::new("dash", Ty::list(Ty::Float)).default());
            }
            method(params, Ty::Void)
        }
        "beginLayer" => method(vec![P::new("blend", BLEND_MODE).default(), f("opacity").default()], Ty::Void),
        "endLayer" => method(vec![], Ty::Void),
        "scale" => method(vec![u("x"), u("y")], Ty::Void),
        "rotate" => method(vec![u("angle")], Ty::Void),
        "zoom" => method(vec![u("factor")], Ty::Void),
        "clip" => method(vec![f("x"), f("y"), f("width"), f("height")], Ty::Void),
        "clipPath" => method(vec![P::new("evenOdd", Ty::Bool).default()], Ty::Void),
        _ => return None,
    })
}

/// Methods of `Scene`, which a `Scene3D`'s block says what's in it with.
/// Lengths are in the scene's own units, and angles in radians.
fn scene_method(name: &str) -> Option<Method> {
    let f = |l: &str| P::new(l, Ty::Float);
    let u = |l: &str| P::unlabeled(l, Ty::Float);
    let color = || P::new("color", COLOR).default();
    // Where a shape's middle is.
    let at = || [f("x").default(), f("y").default(), f("z").default()];
    let shape = |mut sizes: Vec<P>| {
        sizes.push(color());
        sizes.extend(at());
        sizes.push(f("opacity").default());
        method(sizes, Ty::Void)
    };
    Some(match name {
        "box" => shape(vec![f("width").default(), f("height").default(), f("depth").default()]),
        "sphere" => shape(vec![f("radius").default()]),
        "cylinder" | "cone" => shape(vec![f("radius").default(), f("height").default()]),
        "plane" => shape(vec![f("width").default(), f("depth").default()]),
        "mesh" => method(
            vec![
                P::new("vertices", Ty::list(Ty::Float)),
                P::new("triangles", Ty::list(Ty::Int)).default(),
                color(),
                P::new("colors", Ty::list(COLOR)).default(),
                P::new("smooth", Ty::Bool).default(),
                f("opacity").default(),
            ],
            Ty::Void,
        ),
        "surface" => method(
            vec![
                P::new("heights", Ty::list(Ty::Float)),
                P::new("columns", Ty::Int),
                f("width").default(),
                f("depth").default(),
                color(),
                P::new("colors", Ty::list(COLOR)).default(),
                f("opacity").default(),
            ],
            Ty::Void,
        ),
        "line" => method(vec![u("x1"), u("y1"), u("z1"), u("x2"), u("y2"), u("z2"), color()], Ty::Void),
        "lines" => method(
            vec![P::unlabeled("points", Ty::list(Ty::Float)), color(), P::new("closed", Ty::Bool).default()],
            Ty::Void,
        ),
        "label" => {
            let [x, y, z] = at();
            method(vec![P::unlabeled("text", Ty::Str), x, y, z, f("size").default(), color()], Ty::Void)
        }
        "camera" => method(
            vec![
                f("x"),
                f("y"),
                f("z"),
                f("lookAtX").default(),
                f("lookAtY").default(),
                f("lookAtZ").default(),
                f("fieldOfView").default(),
            ],
            Ty::Void,
        ),
        "orbit" => {
            let [x, y, z] = at();
            method(vec![f("distance"), f("yaw").default(), f("pitch").default(), x, y, z, f("fieldOfView").default()], Ty::Void)
        }
        "light" => method(vec![f("x"), f("y"), f("z"), color(), f("strength").default()], Ty::Void),
        "ambient" => method(vec![u("amount")], Ty::Void),
        "background" => method(vec![P::unlabeled("color", COLOR)], Ty::Void),
        "save" | "restore" => method(vec![], Ty::Void),
        "translate" => method(vec![u("x"), u("y"), u("z")], Ty::Void),
        "scale" => method(vec![u("x"), u("y"), u("z")], Ty::Void),
        "rotateX" | "rotateY" | "rotateZ" => method(vec![u("angle")], Ty::Void),
        _ => return None,
    })
}

/// The method of `Graphics` a call of `name` means: a few draw from
/// something else when their first argument has another label
/// (`g.image(pixels: …)`, `g.svg(text: …)`), and are told apart by it.
pub fn graphics_variant<'a>(name: &'a str, first_label: Option<&str>) -> &'a str {
    match (name, first_label) {
        ("image", Some("pixels")) => "image(pixels:)",
        ("svg", Some("text")) => "svg(text:)",
        _ => name,
    }
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
        Ty::Int => &["isEven", "isOdd", "clamped", "bytes", "wrappingAdd", "wrappingSubtract", "wrappingMultiply"],
        Ty::Dict(..) => &["count", "isEmpty", "keys", "values", "removeValue", "contains"],
        Ty::Set(_) => &[
            "count", "isEmpty", "values", "contains", "insert", "remove", "union", "intersection", "subtracting",
            "isSubset", "isSuperset", "isDisjoint", "filter", "sorted",
        ],
        Ty::Float => &["formatted", "clamped", "isNaN", "isFinite", "bytes"],
        Ty::Data => &[
            "count", "isEmpty", "bytes", "text", "base64", "hex", "latin1", "slice", "hash", "int8", "int16", "uint16", "int32", "uint32",
            "int64", "float32", "float64", "compressed", "decompressed", "append",
        ],
        t if *t == GRAPHICS => &[
            "width", "height", "beginPath", "moveTo", "lineTo", "quadTo", "curveTo", "closePath", "lines", "rect", "circle",
            "ellipse", "arc", "fill", "stroke", "line", "fillRect", "strokeRect", "fillCircle", "strokeCircle", "text",
            "textWidth", "textPath", "image", "translate", "scale", "rotate", "zoom", "resetTransform", "save", "restore", "clip", "clipPath",
            "fillLinear", "fillRadial", "strokeLinear", "strokeRadial", "beginLayer", "endLayer", "svg", "newPage",
        ],
        t if *t == SCENE => &[
            "width", "height", "camera", "orbit", "light", "ambient", "background", "box", "sphere", "cylinder", "cone",
            "plane", "mesh", "surface", "line", "lines", "label", "translate", "scale", "rotateX", "rotateY", "rotateZ",
            "save", "restore",
        ],
        t if *t == JOB => &["id", "progress", "isCancelled", "cancel"],
        t if *t == SOUND => &["id", "pause", "resume", "stop", "seek", "setVolume", "isPlaying", "position", "duration"],
        Ty::View => &[
            "padding", "font", "color", "background", "border", "frame", "opacity", "hidden", "disabled", "onTap",
            "id", "tag", "shortcut", "onAppear", "onSubmit", "tabItem", "hoverBackground", "autofocus",
            "textAlignment", "animation", "onDrag", "onPointer", "onKey", "cursor", "offset", "clipped", "onDrop",
            "tooltip", "accessibilityLabel", "onResize", "onKeyEvent", "draggable", "dropTarget", "contextMenu", "menuBar", "popover", "sheet", "onOpenFile",
            "onCloseRequest", "gridCellColumns", "gridColumnAlignment", "selectable", "lineLimit",
            "rotation", "scale", "trayIcon", "dragFile",
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
        "fontFamilies" => rt("tsl_font_families"),
        "chooseFolder" => rt_d("tsl_choose_folder", &[Dflt::Str("")]),
        "__openFiles" => rt("tsl_open_files"),
        "__messageDialog" => rt("tsl_message_dialog"),
        "__printFile" => rt("tsl_print_file"),
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
        "appPath" => rt("tsl_app_path"),
        "appFolder" => rt("tsl_app_folder"),
        "__richId" => rt("tsl_rich_id"),
        "__richMarks" => rt("tsl_rich_marks"),
        "__richApply" => rt("tsl_rich_apply"),
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
        "sendToCommand" => rt("tsl_send_to_command"),
        "closeWebSocket" => rt("tsl_close_web_socket"),
        "readData" => rt("tsl_read_data"),
        "writeImage" => rt("tsl_write_image"),
        "__imageSize" => rt("tsl_image_size"),
        "__appInfo" => rt("tsl_app_info"),
        "__imagePixels" => rt("tsl_image_pixels"),
        "__dbOpen" => rt("tsl_db_open"),
        "__dbClose" => rt("tsl_db_close"),
        "__dbLastInsertId" => rt("tsl_db_last_insert_id"),
        "__dbExecute" => rt("tsl_db_execute"),
        "__dbQuery" => rt("tsl_db_query"),
        "__dbNoColumn" => rt("tsl_db_no_column"),
        "setWindowTitle" => rt("tsl_set_window_title"),
        "setFullScreen" => rt("tsl_set_full_screen"),
        "showWindow" => rt("tsl_show_window"),
        "__playSound" => rt("tsl_play_sound"),
        "showNotification" => rt_d("tsl_show_notification", &[Dflt::Req, Dflt::Str("")]),
        "isFullScreen" => rt("tsl_is_full_screen"),
        "undoTextEdit" => rt("tsl_undo_text_edit"),
        "redoTextEdit" => rt("tsl_redo_text_edit"),
        "quit" => rt("tsl_quit"),
        "textOutline" => rt_d("tsl_text_outline", &[Dflt::Req, Dflt::Req, Dflt::Case(0), Dflt::Bool(false), Dflt::Str(""), Dflt::Bool(true), Dflt::Float(0.0)]),
        "__textGlyphs" => rt_d("tsl_text_glyphs", &[Dflt::Req, Dflt::Req, Dflt::Case(0), Dflt::Bool(false), Dflt::Str(""), Dflt::Bool(true), Dflt::Float(0.0)]),
        "__fontMetrics" => rt_d("tsl_font_metrics", &[Dflt::Req, Dflt::Case(0), Dflt::Bool(false), Dflt::Str("")]),
        "__fontStyleNames" => rt("tsl_font_style_names"),
        "__fontStyleKinds" => rt("tsl_font_style_kinds"),
        "__permission" => rt("tsl_permission"),
        "startLocation" => rt_d("tsl_location_start", &[Dflt::Bool(true), Dflt::Float(1.0)]),
        "stopLocation" => rt("tsl_location_stop"),
        "__location" => rt("tsl_location"),
        "__satellites" => rt("tsl_satellites"),
        "startCompass" => rt("tsl_compass_start"),
        "stopCompass" => rt("tsl_compass_stop"),
        "__heading" => rt("tsl_heading"),
        "closeWindow" => rt("tsl_close_window"),
        "dismiss" => rt("tsl_ui_dismiss"),
        "showNotice" => rt_d("tsl_ui_notice", &[Dflt::Req, Dflt::Float(2.5)]),
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
        (Ty::Int, "wrappingAdd") => rt("tsl_int_wrapping_add"),
        (Ty::Int, "wrappingSubtract") => rt("tsl_int_wrapping_sub"),
        (Ty::Int, "wrappingMultiply") => rt("tsl_int_wrapping_mul"),
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
        // (An algorithm it doesn't know stops the program.)
        (Ty::Data, "hash") => RtCall { rt: "tsl_data_hash", defaults: &[Dflt::Str("sha256")], located: true, no_receiver: false },
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
        (t, _) if *t == SCENE => return scene_call(name),
        // (The receiver goes as its `id`.)
        (t, "progress") if *t == JOB => rt_d("tsl_job_progress", &[Dflt::Req, Dflt::Str("")]),
        (t, "isCancelled") if *t == JOB => rt("tsl_job_is_cancelled"),
        (t, "cancel") if *t == JOB => rt("tsl_job_cancel"),
        (t, "pause") if *t == SOUND => rt("tsl_sound_pause"),
        (t, "resume") if *t == SOUND => rt("tsl_sound_resume"),
        (t, "stop") if *t == SOUND => rt("tsl_sound_stop"),
        (t, "seek") if *t == SOUND => rt("tsl_sound_seek"),
        (t, "setVolume") if *t == SOUND => rt("tsl_sound_set_volume"),
        (t, "isPlaying") if *t == SOUND => rt("tsl_sound_is_playing"),
        (t, "position") if *t == SOUND => rt("tsl_sound_position"),
        (t, "duration") if *t == SOUND => rt("tsl_sound_duration"),
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
            &[Req, Req, Req, Float(14.0), PRIMARY, REGULAR, TOP_LEADING, Float(0.0), CLEAR, Float(0.0), Bool(false), Dflt::Str(""), Bool(true), Float(0.0)],
        ),
        "textWidth" => rt_g("tsl_canvas_text_width", &[Req, Float(14.0), REGULAR, Bool(false), Dflt::Str(""), Bool(true), Float(0.0)]),
        "textPath" => rt_g(
            "tsl_canvas_text_path",
            &[Req, Req, Req, Float(14.0), REGULAR, TOP_LEADING, Bool(false), Dflt::Str(""), Bool(true), Float(0.0)],
        ),
        "image" => rt_g("tsl_canvas_image", &[Req, Req, Req, Req, Req, Float(1.0)]),
        "image(pixels:)" => rt_g("tsl_canvas_image_pixels", &[Req, Req, Req, Req, Req, Req, Req, Float(1.0)]),
        "svg" => rt_g("tsl_canvas_svg", &[Req, Req, Req, Req, Req, PRIMARY, Float(1.0)]),
        "svg(text:)" => rt_g("tsl_canvas_svg_text", &[Req, Req, Req, Req, Req, PRIMARY, Float(1.0)]),
        "fillLinear" => rt_g("tsl_canvas_fill_linear", &[Req, Req, Req, Req, Req, EmptyList, Bool(false)]),
        "fillRadial" => rt_g("tsl_canvas_fill_radial", &[Req, Req, Req, Req, EmptyList, Bool(false)]),
        "strokeLinear" => rt_g("tsl_canvas_stroke_linear", &[Req, Req, Req, Req, Req, EmptyList, Float(1.0), EmptyList]),
        "strokeRadial" => rt_g("tsl_canvas_stroke_radial", &[Req, Req, Req, Req, EmptyList, Float(1.0), EmptyList]),
        "beginLayer" => rt_g("tsl_canvas_begin_layer", &[Case(0), Float(1.0)]),
        "endLayer" => rt_g("tsl_canvas_end_layer", &[]),
        "newPage" => rt_g("tsl_canvas_new_page", &[Float(0.0), Float(0.0)]),
        "clip" => rt_g("tsl_canvas_clip", &[]),
        "clipPath" => rt_g("tsl_canvas_clip_path", &[Bool(false)]),
        _ => return None,
    })
}

/// The runtime calls behind the methods of `Scene` (see `scene_method`).
fn scene_call(name: &str) -> Option<RtCall> {
    use Dflt::{Bool, Case, EmptyList, Float, Req};
    // `Color.gray`, `.primary` and `.white`.
    const GRAY: Dflt = Case(2);
    const PRIMARY: Dflt = Case(0);
    const WHITE: Dflt = Case(10);
    const AT: Dflt = Float(0.0);
    const WHOLE: Dflt = Float(1.0);
    Some(match name {
        "box" => rt_g("tsl_scene_box", &[Float(1.0), Float(1.0), Float(1.0), GRAY, AT, AT, AT, WHOLE]),
        "sphere" => rt_g("tsl_scene_sphere", &[Float(0.5), GRAY, AT, AT, AT, WHOLE]),
        "cylinder" => rt_g("tsl_scene_cylinder", &[Float(0.5), Float(1.0), GRAY, AT, AT, AT, WHOLE]),
        "cone" => rt_g("tsl_scene_cone", &[Float(0.5), Float(1.0), GRAY, AT, AT, AT, WHOLE]),
        "plane" => rt_g("tsl_scene_plane", &[Float(1.0), Float(1.0), GRAY, AT, AT, AT, WHOLE]),
        "mesh" => rt_g("tsl_scene_mesh", &[Req, EmptyList, GRAY, EmptyList, Bool(false), WHOLE]),
        "surface" => rt_g("tsl_scene_surface", &[Req, Req, Float(1.0), Float(1.0), GRAY, EmptyList, WHOLE]),
        "line" => rt_g("tsl_scene_line", &[Req, Req, Req, Req, Req, Req, PRIMARY]),
        "lines" => rt_g("tsl_scene_lines", &[Req, PRIMARY, Bool(false)]),
        "label" => rt_g("tsl_scene_label", &[Req, AT, AT, AT, Float(12.0), PRIMARY]),
        "camera" => rt_g("tsl_scene_camera", &[Req, Req, Req, AT, AT, AT, Float(45.0)]),
        "orbit" => rt_g("tsl_scene_orbit", &[Req, Float(0.6), Float(0.4), AT, AT, AT, Float(45.0)]),
        "light" => rt_g("tsl_scene_light", &[Req, Req, Req, WHITE, Float(1.0)]),
        "ambient" => rt_g("tsl_scene_ambient", &[]),
        "background" => rt_g("tsl_scene_background", &[]),
        "save" => rt_g("tsl_scene_save", &[]),
        "restore" => rt_g("tsl_scene_restore", &[]),
        "translate" => rt_g("tsl_scene_translate", &[]),
        "scale" => rt_g("tsl_scene_scale", &[]),
        "rotateX" => rt_g("tsl_scene_rotate_x", &[]),
        "rotateY" => rt_g("tsl_scene_rotate_y", &[]),
        "rotateZ" => rt_g("tsl_scene_rotate_z", &[]),
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
        (Ty::Data, "hex") => rt("tsl_data_hex"),
        (Ty::Data, "count") => rt("tsl_data_len"),
        (Ty::Data, "isEmpty") => rt("tsl_data_is_empty"),
        (Ty::Data, "bytes") => rt("tsl_data_ints"),
        (Ty::Data, "text") => rt("tsl_data_text"),
        (Ty::Data, "latin1") => rt("tsl_data_latin1"),
        _ => return None,
    })
}
