---
title: State and bindings
description: How state makes a Tessel UI change, and how bind parameters let views edit values that belong to someone else.
---

A Tessel UI is a picture of your data. You keep the data that can change in
`state`, and the window always shows its current value.

```tessel
app Counter(width: 320, height: 200) {
    state count = 0

    VStack(spacing: 12) {
        Text("Count: {count}")
            .font(size: 32, weight: .bold)

        HStack(spacing: 8) {
            Button("-") { count -= 1 }
            Button("+") { count += 1 }
        }
    }
}
```

## Declaring state

`state name = value` declares a value that belongs to the app or view and can
change over time. Its type comes from the starting value, just like `var`.
When the starting value doesn't say enough (an empty list, or `nil`), write
the type:

```tessel
state todos: [String] = []
state selected: String? = nil
state volume = 0.5
state mode = Mode.list
```

`state` must be declared directly in the body of an `app` or `view`, not
inside a `VStack` or other block. Anything that can hold state can change it:
button actions, functions in the body, callbacks from
[timers and background work](/ui/async/), and controls like `TextField` and
`Toggle`.

## How changes update the window

You never tell Tessel *what* to redraw. After anything that may have changed
state (a click, a key press, a timer, a finished download), Tessel runs your
app's body again from the top, builds a fresh description of the whole UI, and
draws that.

That's why a body is written as a plain description:

```tessel
if count > 10 {
    Text("That's a lot!").color(.red)
}
```

When `count` passes 10 the text appears; when it drops back, it disappears.
There is no code to show or hide it.

State survives these rebuilds. Everything else in the body (a `let`, the
result of a function call) is computed fresh each time, so it can never get
out of date.

## Bindings: `bind` parameters

Normal parameters are copies: a view can read them but not change them. Often,
though, a view needs to *edit* something that belongs to its caller. A text
field edits a string; a row in a to-do list ticks off a to-do. For that, a
parameter is declared with `bind`:

```tessel
view Stepper(label: String, value: bind Int) {
    HStack(spacing: 8) {
        Text("{label}: {value}")
        Button("-") { value -= 1 }
        Button("+") { value += 1 }
    }
}

app Order(width: 320, height: 200) {
    state apples = 0
    state pears = 2

    Stepper(label: "Apples", value: apples)
    Stepper(label: "Pears", value: pears)
    Text("{apples + pears} pieces of fruit")
}
```

A `bind` parameter isn't a copy: it *is* the caller's value. When the stepper
changes `value`, it changes `apples` in the app, and the total updates.

There's no special marker at the call site: you write `value: apples`, and the
parameter's declaration decides that it's a binding. The built-in controls work
the same way: `TextField`'s `text:`, `Toggle`'s `isOn:`, `Picker`'s and
`TabView`'s `selection:` and `CodeEditor`'s `text:` and `line:` are all `bind`
parameters.

### What can be bound

Only something that can be changed can be passed to a `bind` parameter:

- a `state`, or a local `var`;
- another `bind` parameter (bindings can be passed on through any number of views);
- a field or list element of one of those, like `todo.done`, `settings[0]`
  or `todos[i].title`;
- the loop variable of a `for` loop over one of those (see below).

A value that was just computed can't be bound, because there would be nowhere
to put the change:

```text
error: `TextField`'s `text:` needs something it can change
  |
3 |     TextField("Name", text: name.uppercase())
  |                             ^^^^^^^^^^^^^^^^ this is a value, not a variable
  |
  = help: pass a `state` or `var`, or a field of one
```

`bind` is only allowed on view parameters. Functions always get copies.

### Binding struct fields and list elements

Bindings reach inside structs and lists. Here each row edits one `Todo` in the
app's list, and the toggle inside it edits just that to-do's `done` field:

```tessel
struct Todo {
    title: String
    done: Bool = false
}

view TodoRow(todo: bind Todo) {
    HStack(spacing: 8) {
        Toggle(isOn: todo.done)
        Text(todo.title)
            .color(if todo.done { .gray } else { .primary })
        Spacer()
    }
}

app Todos(width: 360, height: 240) {
    state todos = [Todo(title: "Write the spec"), Todo(title: "Ship it")]

    for todo in todos {
        TodoRow(todo: todo)
    }
    Text("{todos.filter { t in !t.done }.count} left")
}
```

When a `for` loop goes over something bindable (here the `todos` state), its
loop variable is bindable too: `todo` stands for that element of the list, so
`TodoRow(todo: todo)` edits the real list, and `Toggle(isOn: todo.done)` edits
one field of one element.

If you loop over a computed list instead, such as
`todos.filter { t in !t.done }`, the loop variable is a plain copy and can't be
bound. To show only some items and still edit them, loop over the whole list
and use `if` inside the loop:

```tessel
for todo in todos {
    if !todo.done {
        TodoRow(todo: todo)
    }
}
```

A whole list can be bound as well, and passed on:

```tessel
struct Setting {
    name: String
    on: Bool = false
}

view Settings(items: bind [Setting]) {
    for item in items {
        Toggle(item.name, isOn: item.on)
    }
}

app Prefs(width: 320, height: 200) {
    state settings = [Setting(name: "Wi-Fi"), Setting(name: "Sound", on: true)]

    Settings(items: settings)
}
```

## State in your own views

A custom `view` can have its own `state`, which works just like the app's:

```tessel
view Expander(title: String, details: String) {
    state open = false

    VStack(spacing: 4, alignment: .leading) {
        Button(if open { "Hide {title}" } else { "Show {title}" }) { open = !open }
        if open {
            Text(details).color(.secondary)
        }
    }
}

app Faq(width: 360, height: 260) {
    Expander(title: "shipping", details: "Orders ship within two days.")
    Expander(title: "returns", details: "Returns are free for 30 days.")
}
```

Each time a view is shown, that *instance* gets its own copy of the state: the
two expanders above open and close separately.

A few things to know about view state:

- **The starting value is used once**, when the view first appears. It may use
  the view's parameters (`state draft = title`), but later changes to the
  parameter don't reset it.
- **It lasts as long as the view is shown.** When an `if` stops showing a view,
  or its item is removed from a `for` loop, its state is thrown away. If it
  comes back, it starts fresh.
- **It's tied to the view's position in the UI.** In a `for` loop, instances
  are matched to items by position unless you give them an identity with
  `.id(…)`; see [Identity with `.id`](/ui/lists/#identity-with-id).

### State or binding?

Use `state` for things only this view cares about, like whether a section is
expanded. Use a `bind` parameter when the value really belongs to the caller
(so other parts of the UI can see it too) and this view just edits it.
