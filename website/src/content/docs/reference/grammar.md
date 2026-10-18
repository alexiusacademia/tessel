---
title: Grammar
description: The complete syntax of Tessel, in EBNF-style notation, with operator precedence and the rules for lines and blocks.
---

This page describes Tessel's syntax precisely. It follows the compiler's
lexer and parser; the [language guide](/language/basics/) explains the same
things with examples. Rules the type checker adds on top of the syntax (for
example, where `state` may appear) are listed at the end.

## Notation

```text
rule    = definition ;       a rule
"if"                         a keyword or symbol, written exactly
IDENT, INT, …                a token described under "Lexical elements"
a b                          a followed by b
a | b                        a or b
( … )                        grouping
a?                           optional a
a*                           zero or more a
a+                           one or more a
NL                           a line break (see "Lines")
```

## Lexical elements

Source files are UTF-8 text in files ending in `.tsl`.

### Whitespace and comments

Spaces, tabs and carriage returns separate tokens and are otherwise ignored.
Line breaks are significant (see [Lines](#lines)).

```text
comment = "//" (any character except a line break)* ;
```

There are no block comments. A comment runs to the end of the line.

### Identifiers

```text
IDENT = start_char name_char* ;          (but not a keyword)
start_char = "_" | "A"…"Z" | "a"…"z" | any non-ASCII letter ;
name_char  = start_char | "0"…"9" | any non-ASCII letter or digit ;
```

Identifiers are case-sensitive. By convention, types and views use
`UpperCamelCase` and everything else `lowerCamelCase`. `_` on its own is not
an identifier (see below).

### Keywords

```text
app  view  struct  enum  interface  fn
let  var  state  bind
if  else  for  while  in  match  return  try  break  continue
true  false  nil
```

`import`, `public` and `private` are keywords only where they begin an
import or a declaration (or a member, for `private`); elsewhere they can be
used as names. So are `async` (before `fn`), `static` (before `fn` or `let`
in a struct or enum) and `await` (before a call).

`_` is also reserved: it marks an unlabeled parameter, ignores a closure
parameter, a `for` loop variable or a value in a `match` pattern, and is the
catch-all pattern.

These names are not keywords but are built in and can't be declared again:
the types `Int`, `Float`, `Bool`, `String`, `Range`, `View` and `Set`; the enums `Color`,
`Weight`, `Alignment` and `IconName`; the [built-in views](/reference/views/);
and the [built-in functions](/reference/stdlib/). The built-in constant
[`pi`](/reference/stdlib/#math) isn't reserved: a variable or function of your
own named `pi` hides it. Inside methods, `self` is an ordinary name that
refers to the value the method was called on.

### Numbers

```text
INT      = digits
         | "0x" hex_digit (hex_digit | "_")*
         | "0b" ("0" | "1") ("0" | "1" | "_")*
         | "0o" "0"…"7" ("0"…"7" | "_")* ;
FLOAT    = digits "." digits exponent?
         | digits exponent ;
exponent = ("e" | "E") ("+" | "-")? digits ;
digits   = digit (digit | "_")* ;
digit    = "0"…"9" ;
hex_digit = digit | "a"…"f" | "A"…"F" ;
```

- `_` can separate digits for readability: `1_000_000`, `0xFF_FF`. It is
  ignored.
- `0x`, `0b` and `0o` start a hexadecimal, binary or octal `INT`; the prefix
  is lowercase. `0xff` is `255`.
- An `INT` must fit in a 64-bit signed integer (at most
  `9_223_372_036_854_775_807`, or `0x7fff_ffff_ffff_ffff`).
- A `FLOAT` needs digits on both sides of the point: `0.5`, not `.5` or
  `5.`. A number with an exponent is a `FLOAT`: `1e6`, `2.5e-3`, `1E+9`.
- Negative numbers are written with the prefix operator `-`.

### Strings

```text
STRING        = '"' (string_char | escape | interpolation)* '"' ;
string_char   = any character except '"', '\', '{' and a line break ;
escape        = '\n' | '\r' | '\t' | '\"' | '\\' | '\{' | '\}' ;
interpolation = "{" expr "}" ;
```

- A string must end on the line it starts on.
- `{expr}` inserts the value of an expression; the expression can contain
  strings and braces of its own, but no line breaks: `"Total: {count}"`,
  `"{name ?? "none"}"`. An empty `{}` is an error.
- To write a brace itself, use `\{` (and optionally `\}`).
- Only `Int`, `Float`, `Bool`, `String` and enum values can be interpolated.

### Symbols

```text
(  )  {  }  [  ]  ,  :  .  ..  ?  ?.  ??  ->
=  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=
+  -  *  /  %  ==  !=  <  <=  >  >=  &&  ||  !
&  |  ^  ~  <<  >>
```

A `;` is an error: Tessel doesn't use semicolons.

`>>` is two `>` tokens with nothing between them (and `>>=` is `>` followed
by `>=`), so the `>>` that closes `Stack<Stack<Int>>` isn't a shift. A shift
right must be written without a space inside it.

### Lines

Line breaks end declarations, statements and match arms. The lexer produces a
`NL` token for a line break, with these rules:

- Several line breaks in a row (including blank lines and lines with only a
  comment) make one `NL`.
- **Inside `(…)` and `[…]`, line breaks are ignored**, so argument lists and
  list literals can span lines. A `{…}` inside them makes line breaks count
  again until it is closed.

The parser also lets an expression continue onto the next line in these
places (shown as `NL*` in the rules below):

- after a binary operator (`||`, `&&`, comparisons, `??`, `+`, `-`, `*`, `/`,
  `%`, `&`, `|`, `^`, `<<`, `>>`), but not after `..`;
- after `=` in a declaration or assignment, and after the other assignment
  operators (`+=`, `-=`, …);
- before `else`, which may start the line after an `if` block's `}`;
- before a `.` at the start of a line: a line starting with `.name`
  continues the expression above it. This is how modifier chains span lines.
  The exception is a line that is a `match` arm (it contains a `->` outside
  brackets), such as `.done -> …`, or that starts one whose patterns go on
  over several lines (it ends with a `,`).

## Declarations

A program is a folder of `.tsl` files. Each file is a list of imports and
declarations:

```text
file   = NL? (entry (NL entry)*)? NL? ;
entry  = import | visibility? item ;
import = "import" (IDENT | STRING) ("as" IDENT)? ;
visibility = "public" | "private" ;
item   = app_decl | view_decl | struct_decl | enum_decl | interface_decl | fn_decl
       | const_decl ;
const_decl = "let" IDENT (":" type)? "=" expr ;
```

A `let` at the top level is a [constant](/language/basics/#constants),
worked out once when the program starts.

`import geometry` loads the module in the folder `geometry` next to the
file, or else the [library](/guides/libraries/) `geometry` the project
uses; a string is a path to the folder, relative to the file. `as` gives
the module another name in the file: `import geometry as geo`. `public`
makes a module's declaration usable by the code that imports the module,
and `private` limits a declaration to its own file. An `app` can be neither.
See [Programs and files](/language/programs/#modules).

### app and view

```text
app_decl  = "app" IDENT ("(" args? ")")? block ;
view_decl = "view" IDENT params? block ;
```

The `app`'s arguments are `title:`, `width:`, `height:` and `appearance:` (see
[`app`](/reference/views/#app)). A view without parameters may leave out the
parentheses: `view Header { … }`.

### Functions and parameters

```text
fn_decl = "async"? "fn" IDENT generics? params ("->" type)? block ;
params  = "(" (param ("," param)* ","?)? ")" ;
param   = "_"? IDENT ":" "bind"? type ("=" expr)? ;
```

- `_` before the name means callers pass the argument without a label.
- `bind` (only allowed in a `view`'s parameters) makes the parameter a
  binding to the caller's value.
- `= expr` gives a default value. Defaults are evaluated where the call is
  made and can only use global names.
- A function needs parentheses even with no parameters: `fn reset() { … }`.
- An `async fn` can use `await`; see
  [Waiting with async and await](/language/async/). It can't have `bind`
  parameters.

### struct

```text
struct_decl = "struct" IDENT generics? conforms? "{" members "}" ;
generics    = "<" IDENT (":" type_name)? ("," IDENT (":" type_name)?)* ">" ;
conforms    = ":" type_name ("," type_name)* ;
type_name   = IDENT ("." IDENT)? ;                  a name, or `module.Name`
field       = IDENT ":" type ("=" expr)? ;
computed    = IDENT ":" type block ;
static      = "static" (fn_decl | const_decl) ;
members     = (separator* visibility? (field | computed | static | fn_decl))* separator* ;   (for structs)
separator   = NL | "," ;
```

Fields and methods are separated by line breaks or commas:
`struct Point { x: Float, y: Float }`. A
[computed property](/language/structs-enums/#computed-properties) is a type
followed by a block (which starts on the same line), and a
[static member](/language/structs-enums/#static-members) is used through the
type's name. A generic struct or enum can't have static members.

### enum

```text
enum_decl = "enum" IDENT generics? conforms? "{" members "}" ;
case      = IDENT params? ;
members   = (separator* (case | visibility? (computed | static | fn_decl)))* separator* ;      (for enums)
```

A case may carry values, declared like parameters:
`enum Shape { circle(radius: Float), square(side: Float) }`.

### interface

```text
interface_decl = "interface" IDENT "{" (separator* requirement)* separator* "}" ;
requirement    = "async"? "fn" IDENT params ("->" type)? | IDENT ":" type ;
```

A requirement is a method's signature without a body, or a property (a
field every conforming struct has). Its parameters can't
have default values. See [Interfaces](/language/interfaces/).

## Types

```text
type      = base_type "?"* ;
base_type = type_name                               named type
          | type_name "<" type ("," type)* ">"      generic type
          | "[" type "]"                            list
          | "[" type ":" type "]"                   dictionary
          | "(" type ")"                            the same type, grouped
          | "(" type ("," type)+ ","? ")"           tuple, 2 to 8 items
          | "fn" "(" (type ("," type)* ","?)? ")" ("->" type)? ;   function
```

- Named types are `Int`, `Float`, `Bool`, `String`, `Range`, `View`, the
  built-in enums, and your own structs, enums and interfaces. A generic struct
  or enum is written with its type arguments: `Stack<Int>`. See
  [Generics](/language/generics/). `Set<T>` is the built-in set type.
- A type from a module is written with the module's name: `geometry.Shape`.
- In an expression, `Name<…>` is read as a generic type (like `Stack<Int>()`)
  when it starts with a capital letter and the `>` is followed by `(` or `.`;
  otherwise `<` is "less than".
- `T?` is an optional `T`. In `fn() -> Int?`, the `?` belongs to the return
  type; an optional function is written in parentheses: `(fn() -> Int)?`.
- A function type without `->` returns nothing.

## Blocks and statements

```text
block = "{" NL? (stmt (NL stmt)*)? NL? "}" ;

stmt = local_decl
     | fn_decl
     | return_stmt
     | for_stmt
     | while_stmt
     | "break"
     | "continue"
     | assign_stmt
     | expr ;

local_decl  = ("let" | "var" | "state") IDENT (":" type)? "=" NL* expr
            | ("let" | "var") names "=" NL* expr ;       takes a tuple apart
names       = "(" (IDENT | "_") ("," (IDENT | "_"))+ ")" ;
return_stmt = "return" expr? ;
for_stmt    = "for" (IDENT | "_" | names) "in" header_expr block ;
while_stmt  = "while" condition block ;
assign_stmt = expr assign_op NL* expr ;
assign_op   = "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" ;
```

- Each statement ends at the end of its line (or at the block's `}`). Two
  statements can't share a line.
- A declaration always needs a starting value: `var count = 0`.
- `return`'s value, if any, starts on the same line.
- The loop variable of `for` is a name, or `_` when the body doesn't use it:
  `for _ in 0..3 { … }`.
- Assignment is a statement, not an expression. `x op= v` means
  `x = x op v`.
- A `fn_decl` in a block declares a function that is known from there to the
  end of the block (and in its own body).
- `if` and `match` are expressions, so they can be used as statements too.
- `break` and `continue` apply to the innermost `for` or `while` loop in the
  same function or block. View bodies can't use `while`, `break` or
  `continue`.

## Expressions

### Precedence

From lowest to highest precedence:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `\|\|` | left |
| 2 | `&&` | left |
| 3 | `==` `!=` `<` `<=` `>` `>=` | none: `a < b < c` is an error |
| 4 | `..` | none |
| 5 | `??` | right |
| 6 | `+` `-` <code>&#124;</code> `^` | left |
| 7 | `*` `/` `%` `&` | left |
| 8 | `<<` `>>` | left |
| 9 | prefix `-` `!` `~` `try` | prefix |
| 10 | call `f(…)`, block after a call, `.name`, `?.name`, `[index]` | left |

For example, `a ?? b + 1` means `a ?? (b + 1)`, `0..n + 1` means
`0..(n + 1)`, and `!done && count > 0` means `(!done) && (count > 0)`.
`&&` and `||` only evaluate their right side when needed. The bit operators
are placed as in Swift: `1 + 2 << 3` means `1 + (2 << 3)`, and
`flags & mask == 0` means `(flags & mask) == 0`.

### Grammar

```text
expr        = or_expr ;
or_expr     = and_expr ("||" NL* and_expr)* ;
and_expr    = cmp_expr ("&&" NL* cmp_expr)* ;
cmp_expr    = range_expr (cmp_op NL* range_expr)? ;
cmp_op      = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
range_expr  = coalesce_expr (".." coalesce_expr)? ;
coalesce_expr = add_expr ("??" NL* coalesce_expr)? ;
add_expr    = mul_expr (("+" | "-" | "|" | "^") NL* mul_expr)* ;
mul_expr    = shift_expr (("*" | "/" | "%" | "&") NL* shift_expr)* ;
shift_expr  = unary_expr (("<<" | ">>") NL* unary_expr)* ;
unary_expr  = ("-" | "!" | "~" | "try") unary_expr | "await" postfix_expr | postfix_expr ;

postfix_expr = primary postfix* ;
postfix      = "(" args? ")" closure?          call, optionally with a block after it
             | closure                         block after a name or member (see below)
             | "." (IDENT | INT)               member, or a tuple's item (`.0`)
             | "?." (IDENT | INT)              optional member
             | "[" expr "]"                    index
             | NL "." IDENT                    member on the next line (see "Lines")
             ;

args = arg ("," arg)* ","? ;
arg  = (IDENT ":")? expr ;

primary = INT | FLOAT | STRING | "true" | "false" | "nil"
        | IDENT
        | "." IDENT                            implicit member, like .red or .infinity
        | "(" expr ")"
        | "(" expr ("," expr)+ ","? ")"        tuple, 2 to 8 items
        | list_literal
        | dict_literal
        | closure
        | if_expr
        | match_expr ;

list_literal = "[" (expr ("," expr)* ","?)? "]" ;
dict_literal = "[" ":" "]"
             | "[" expr ":" expr ("," expr ":" expr)* ","? "]" ;

closure        = "{" (closure_params "in")? NL? (stmt (NL stmt)*)? NL? "}" ;
closure_params = (IDENT | "_") ("," (IDENT | "_"))* ;
```

A list literal where a `Set` is expected makes a set:
`let seen: Set<Int> = [1, 2]`.

- The value of a range `a..b` is the `Int`s from `a` up to, but not
  including, `b`. Its type is `Range`.
- A `[…]` literal is a list or a dictionary, depending on whether its first
  entry has a `:`. Entries can't be mixed.
- `.name` on its own is an enum case whose type comes from the context, as
  in `.color(.red)`, or `.infinity` where a `Float` is expected.
- A closure starting with `{ a, b in` takes parameters; one without `in`
  takes none. Its last line is its value.

### Calls and blocks after calls

A call's arguments go in parentheses, each with an optional label:
`greet(name: "Ada")`, `Text("Hi")`. A `{…}` right after the call, **on the
same line**, is passed as the last argument:

```tessel
Button("Save") {
    save()
}
```

A block can also follow a bare name or a member directly, without
parentheses: `VStack { … }`, `numbers.filter { n in n > 0 }`. Only names and
members can take a block this way, so `x + y { … }` isn't a call.

In the header of an `if`, `for` or `match` (the part before its `{`), a `{`
never starts a block after a call, because it starts the body. Put the block
in parentheses there:

```tessel
for n in numbers.filter({ n in n > 0 }) {
    print(n)
}
```

Inside `(…)`, `[…]` or a closure in a header, blocks after calls work
normally again.

Modifier chains are ordinary member calls: `Text("Hi").padding(8)`, and a
line starting with `.` continues the chain:

```tessel
Text("Hi")
    .font(size: 18)
    .padding(8)
```

### if

```text
if_expr     = "if" condition block (NL? "else" (if_expr | block))? ;
condition   = header_expr
            | "let" IDENT (":" type)? "=" header_expr
            | "let" names "=" header_expr ;          takes an optional tuple apart
header_expr = expr ;    (in which a "{" never starts a block after a call)
```

`if let name = value { … }` runs the block when `value` (an optional) isn't
`nil`, with `name` set to the unwrapped value. `if let name: T = value`
also gives the unwrapped type, which `fromJson` and `fromBinary` need.
`if let (a, b) = value` unwraps an optional tuple and names its items
(`_` skips one). `while` takes the same conditions.
There is no `? :` operator; `if` is an expression, so
`let size = if big { 24 } else { 14 }` works.

### match

```text
match_expr = "match" header_expr "{" NL? (arm (NL arm)*)? NL? "}" ;
arm        = pattern ("," NL? pattern)* "->" (block | expr) ;

pattern    = "." IDENT ("(" (binding ("," binding)* ","?)? ")")?    enum case
           | "_"                                                    anything
           | INT | FLOAT | STRING | "true" | "false" | "nil"        literal
           | "-" unary_expr ;                                       negative number
binding    = IDENT | "_" ;
```

- Each arm is on its own line. After `->`, a `{` starts a block (not a
  closure).
- `.circle(r)` matches the case and names its values, in order; `_` skips a
  value.
- An arm with several patterns (`.north, .south -> …`) runs when any of them
  matches. Its patterns can't name values (only `_` is allowed in their
  parentheses).
- A `match` must cover every case: every enum case, both `true` and `false`,
  or a final `_` arm for other types. Arms after a `_` arm are an error.

## Rules beyond the syntax

The grammar accepts some programs that the type checker then rejects. The
most important rules:

- A program needs exactly one entry point: one `app`, or a `fn main()` with
  no parameters and no return type, but not both.
- All top-level names in the program's folder share one namespace and must
  be unique (a module's names are separate: they're written `module.name`).
  `let`, `var` and `state` can't be declared at the top level.
- Outside a module, only its `public` declarations can be used; a `private`
  declaration or member can only be used in its own file.
- `state` can only be declared directly in the body of a `view` or `app`.
- Functions can be declared at the top level, as methods in a `struct` or
  `enum`, directly in the body of a `view` or `app`, and inside functions,
  methods and blocks (where they can't be generic). They can't be declared in
  a nested block of a view body.
- Bit operators (`&`, `|`, `^`, `~`, `<<`, `>>`) work on `Int`s only; `%`
  works on `Int` and `Float`; `+` also joins `String`s, lists and `Data`.
- In a view body (and in the content block of a stack or other container),
  every statement must produce a view, except `let`, `state`, `fn`, and
  `if`, `match` and `for`, whose blocks follow the same rules. `var`,
  assignments and `return` aren't allowed there.
- A block whose type is expected to be a value must end with an expression of
  that type (or a `return`).
- `bind` parameters are only allowed on views, and a `bind` argument must be
  something that can be changed: a `var`, `state`, `bind` parameter, or a
  field or item of one.
