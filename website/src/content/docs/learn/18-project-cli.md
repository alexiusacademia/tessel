---
title: "18. Project: a to-do list in the terminal"
description: Build a complete command-line to-do list step by step, with structs, an enum of commands, a loop that reads input, saving to JSON, and a program split into files.
---

Time to build something complete. In this project you'll write a to-do list
that runs in the terminal. You type commands like `add Buy milk` or
`done 2`, and it keeps your list, even after you quit.

It's a small program, but it has the same parts as big ones: a data model,
input to read and understand, actions that change the data, and storage.

In this lesson you'll learn:

- how to plan a program before writing it
- how to read commands in a loop until the user quits
- how to turn text into an enum, so the rest of the program never deals
  with raw text
- how to keep the data in a struct with methods
- how to save after every change, and split the finished program into files

Here's what using it will look like when you're done:

```text
Your to-do list. Type "help" to see the commands.
add Buy milk
Added "Buy milk".
add Call grandma
Added "Call grandma".
done 1
Well done!
list
1. [x] Buy milk
2. [ ] Call grandma
quit
Bye!
```

## The plan

Before writing code, decide what the program needs. A good way is to
describe it in plain sentences, then look at the nouns and the verbs.

*The user has a list of **tasks**. Each task has a **title** and is
**done** or not. The user can **add** a task, **list** the tasks, mark one
as **done**, **remove** one, ask for **help**, and **quit**. The list is
**saved** between runs.*

- The nouns become data: a `Task` struct with `title` and `done`, and a list
  of tasks.
- The verbs become commands: `add`, `list`, `done`, `remove`, `help`,
  `quit`. A fixed set of choices is exactly what an enum is for.
- "Saved between runs" means a JSON file, as in
  [lesson 16](/learn/16-files-data/).

You'll build it in small steps, running the program after each one. Small
steps mean that when something goes wrong, you know it's in the few lines
you just wrote.

## Step 1: the tasks

Make a folder called `todo`, and in it a file called `main.tsl`. Start with
the data, and a way to print it:

```tessel
struct Task {
    title: String
    done: Bool = false
}

fn showTasks(_ tasks: [Task]) {
    for i in tasks.indices {
        let task = tasks[i]
        let mark = if task.done { "x" } else { " " }
        print("{i + 1}. [{mark}] {task.title}")
    }
}

fn main() {
    var tasks = [Task(title: "Buy milk"), Task(title: "Water the plants")]
    tasks[1].done = true
    showTasks(tasks)
}
```

```sh
tessel run todo
```

```text
1. [ ] Buy milk
2. [x] Water the plants
```

The list counts from 1, because that's how people count. In the code,
indexes still start at 0, so the number shown is `i + 1`. You'll need to
remember that when the user types `done 2`: task 2 is at index 1.

## Step 2: reading commands

A command-line program usually works in a loop: read a line, do something,
repeat. `readLine()` returns `nil` when the input ends, so `while let` reads
every line until then. Replace `main` with this, to try it out:

```tessel
fn main() {
    print("Type something (or quit):")
    while let line = readLine() {
        if line.trim() == "quit" {
            break
        }
        print("You typed \"{line}\".")
    }
    print("Bye!")
}
```

Run it and type a few lines. Instead of typing, you can also *pipe* lines
into the program with `printf`, where each `\n` is an Enter. That's handy
for trying the same input again and again:

```sh
printf 'hello\nadd Buy milk\nquit\nthis is never read\n' | tessel run todo
```

```text
Type something (or quit):
You typed "hello".
You typed "add Buy milk".
Bye!
```

`break` leaves the loop at `quit`, so the last line is never read. The loop
also ends by itself when the input runs out (or when you press Ctrl-D in
the terminal), because `readLine()` then returns `nil`.

## Step 3: understanding commands

Now for the interesting part. The user types text, like `done 2`. You could
check the text with `hasPrefix("done")` all over the program, but that gets
messy fast. Instead, turn each line into a value of an enum as soon as it's
read. The rest of the program then works with clear, checked values, and
`match` makes sure you handle every command.

```tessel
enum Command {
    add(title: String)
    list
    done(number: Int)
    remove(number: Int)
    help
    quit
    unknown(text: String)
}
```

Some cases carry a value: the title to add, or the number of the task.
`unknown` holds whatever the user typed that didn't make sense, so the
program can say what it didn't understand.

Turning text into a structured value like this is called **parsing**. Here's
the parser:

```tessel
fn parseCommand(_ line: String) -> Command {
    let words = line.words()
    let name = (words.first ?? "").lowercase()
    let rest = words.dropFirst().joined(separator: " ")

    match name {
        "add" -> if rest.isEmpty { Command.unknown(text: line) } else { Command.add(title: rest) }
        "list" -> Command.list
        "done" -> numberCommand(line, rest: rest, isDone: true)
        "remove" -> numberCommand(line, rest: rest, isDone: false)
        "help" -> Command.help
        "quit" -> Command.quit
        _ -> Command.unknown(text: line)
    }
}

// "done 2" and "remove 2" need a number after the command.
fn numberCommand(_ line: String, rest: String, isDone: Bool) -> Command {
    if let number = Int(rest) {
        if isDone { Command.done(number: number) } else { Command.remove(number: number) }
    } else {
        Command.unknown(text: line)
    }
}
```

How it works:

- `words()` splits the line at spaces, and ignores extra ones. So
  `"  add   Buy   milk "` becomes `["add", "Buy", "milk"]`.
- The first word is the command's name. `lowercase()` means `DONE 2` works
  too. The other words, joined back together, are the `rest`.
- `match` on a `String` needs a `_` arm for everything else, which becomes
  `unknown`.
- `Int(rest)` returns `nil` if `rest` isn't a number, so `done two` is
  `unknown` too.

Parsers are easy to get subtly wrong, so test this one before using it.
Here's a trick: `toJson` can show an enum case *with* its values, which
`print` alone doesn't. Put this `main` below the parser and run it:

```tessel
fn main() {
    for line in ["add Buy   milk", "DONE 2", "done two", "  list  ", "add"] {
        print(toJson(parseCommand(line)))
    }
}
```

```text
{"add":{"title":"Buy milk"}}
{"done":{"number":2}}
{"unknown":{"text":"done two"}}
"list"
{"unknown":{"text":"add"}}
```

Every line became the command it should.

## Step 4: the list and its actions

The actions change the list: adding, marking done, removing. Functions
can't change their parameters, but a struct's methods can change its
fields. So put the list in a struct, and give it a method for each action:

```tessel
struct TodoList {
    tasks: [Task] = []

    fn add(_ title: String) {
        tasks.append(Task(title: title))
    }

    // Task numbers start at 1, the way people count.
    fn isValid(_ number: Int) -> Bool {
        number >= 1 && number <= tasks.count
    }

    fn markDone(_ number: Int) {
        tasks[number - 1].done = true
    }

    fn remove(_ number: Int) -> Task {
        tasks.remove(at: number - 1)
    }

    fn show() {
        if tasks.isEmpty {
            print("Nothing to do!")
        }
        for i in tasks.indices {
            let task = tasks[i]
            let mark = if task.done { "x" } else { " " }
            print("{i + 1}. [{mark}] {task.title}")
        }
    }
}
```

`show` replaces the `showTasks` function from step 1, so delete that one.
`isValid` matters: `tasks[number - 1]` with a number that's too big would
stop the program with an "index out of range" error. Checking first lets
the program answer politely instead.

Now the main loop reads a line, parses it, and uses `match` to run the
right action:

```tessel
fn main() {
    var list = TodoList()
    print("Your to-do list. Type \"help\" to see the commands.")

    while let line = readLine() {
        let command = parseCommand(line)
        match command {
            .add(title) -> {
                list.add(title)
                print("Added \"{title}\".")
            }
            .list -> list.show()
            .done(number) -> {
                if list.isValid(number) {
                    list.markDone(number)
                    print("Well done!")
                } else {
                    print("There's no task {number}.")
                }
            }
            .remove(number) -> {
                if list.isValid(number) {
                    let task = list.remove(number)
                    print("Removed \"{task.title}\".")
                } else {
                    print("There's no task {number}.")
                }
            }
            .help -> printHelp()
            .quit -> { break }
            .unknown(text) -> print("Sorry, I don't understand \"{text}\". Type \"help\" to see the commands.")
        }
    }
    print("Bye!")
}

fn printHelp() {
    print("Commands:")
    print("  add <title>   add a task")
    print("  list          show all tasks")
    print("  done <n>      mark task n as done")
    print("  remove <n>    remove task n")
    print("  quit          leave the program")
}
```

`.add(title)` in a `match` arm takes the title out of the command and names
it `title`, ready to use. Arms with several lines go in braces.

The file now has `Task`, `TodoList`, `Command`, `parseCommand`,
`numberCommand`, `main` and `printHelp`. Try it:

```sh
printf 'add Buy milk\nadd Water the plants\ndone 1\nlist\nremove 5\nquit\n' | tessel run todo
```

```text
Your to-do list. Type "help" to see the commands.
Added "Buy milk".
Added "Water the plants".
Well done!
1. [x] Buy milk
2. [ ] Water the plants
There's no task 5.
Bye!
```

It works! But quit, run it again, and the list is empty. Time to save.

## Step 5: saving

Saving works just like the high-score table in
[lesson 16](/learn/16-files-data/). Add these functions:

```tessel
fn tasksFile() -> String {
    joinPath(appDataFolder("TodoTutorial"), "tasks.json")
}

fn loadTasks() -> [Task] {
    let text = readFile(tasksFile()) ?? "[]"
    let tasks: [Task] = fromJson(text) ?? []
    tasks
}

fn saveTasks(_ tasks: [Task]) {
    if !writeFile(path: tasksFile(), text: toJson(tasks, pretty: true)) {
        print("Warning: couldn't save your tasks.")
    }
}
```

Then change two things in `main`. Start with the saved tasks:

```tessel
    var list = TodoList(tasks: loadTasks())
```

and save at the end of the loop body, after the `match`, so every command
is saved right away:

```tessel
        saveTasks(list.tasks)
```

Saving after every command, rather than only at `quit`, means nothing is
lost if the user closes the terminal without quitting. The list is small,
so saving it often costs nothing noticeable.

:::tip[Keep your real list safe]
While you're still changing the program, you may not want test runs to mix
with your real to-do list. For testing, make `tasksFile()` return a file in
the current folder, like `"test-tasks.json"`, and switch back to
`appDataFolder` when you're done.
:::

## Step 6: splitting into files

The program works, and it's almost 140 lines long. That's the point where
one file starts to feel crowded. Split it by topic, into four files in the
`todo` folder (see [lesson 16](/learn/16-files-data/) and
[Programs and files](/language/programs/)):

```text
todo/
  task.tsl        Task and TodoList
  commands.tsl    Command, parseCommand, numberCommand, printHelp
  storage.tsl     tasksFile, loadTasks, saveTasks
  main.tsl        main
```

You don't have to change any code: just move each declaration to its file.
Every file sees all the others' names, and `tessel run todo` compiles them
all together. Here is the whole program.

```tessel
// task.tsl: a task, and the list of tasks.

struct Task {
    title: String
    done: Bool = false
}

struct TodoList {
    tasks: [Task] = []

    fn add(_ title: String) {
        tasks.append(Task(title: title))
    }

    // Task numbers start at 1, the way people count.
    fn isValid(_ number: Int) -> Bool {
        number >= 1 && number <= tasks.count
    }

    fn markDone(_ number: Int) {
        tasks[number - 1].done = true
    }

    fn remove(_ number: Int) -> Task {
        tasks.remove(at: number - 1)
    }

    fn show() {
        if tasks.isEmpty {
            print("Nothing to do!")
        }
        for i in tasks.indices {
            let task = tasks[i]
            let mark = if task.done { "x" } else { " " }
            print("{i + 1}. [{mark}] {task.title}")
        }
    }
}
```

```tessel
// commands.tsl: what the user can type, and how to read it.

enum Command {
    add(title: String)
    list
    done(number: Int)
    remove(number: Int)
    help
    quit
    unknown(text: String)
}

fn parseCommand(_ line: String) -> Command {
    let words = line.words()
    let name = (words.first ?? "").lowercase()
    let rest = words.dropFirst().joined(separator: " ")

    match name {
        "add" -> if rest.isEmpty { Command.unknown(text: line) } else { Command.add(title: rest) }
        "list" -> Command.list
        "done" -> numberCommand(line, rest: rest, isDone: true)
        "remove" -> numberCommand(line, rest: rest, isDone: false)
        "help" -> Command.help
        "quit" -> Command.quit
        _ -> Command.unknown(text: line)
    }
}

// "done 2" and "remove 2" need a number after the command.
fn numberCommand(_ line: String, rest: String, isDone: Bool) -> Command {
    if let number = Int(rest) {
        if isDone { Command.done(number: number) } else { Command.remove(number: number) }
    } else {
        Command.unknown(text: line)
    }
}

fn printHelp() {
    print("Commands:")
    print("  add <title>   add a task")
    print("  list          show all tasks")
    print("  done <n>      mark task n as done")
    print("  remove <n>    remove task n")
    print("  quit          leave the program")
}
```

```tessel
// storage.tsl: saving and loading tasks as JSON.

fn tasksFile() -> String {
    joinPath(appDataFolder("TodoTutorial"), "tasks.json")
}

fn loadTasks() -> [Task] {
    let text = readFile(tasksFile()) ?? "[]"
    let tasks: [Task] = fromJson(text) ?? []
    tasks
}

fn saveTasks(_ tasks: [Task]) {
    if !writeFile(path: tasksFile(), text: toJson(tasks, pretty: true)) {
        print("Warning: couldn't save your tasks.")
    }
}
```

```tessel
// main.tsl: read commands until the user quits.

fn main() {
    var list = TodoList(tasks: loadTasks())
    print("Your to-do list. Type \"help\" to see the commands.")

    while let line = readLine() {
        let command = parseCommand(line)
        match command {
            .add(title) -> {
                list.add(title)
                print("Added \"{title}\".")
            }
            .list -> list.show()
            .done(number) -> {
                if list.isValid(number) {
                    list.markDone(number)
                    print("Well done!")
                } else {
                    print("There's no task {number}.")
                }
            }
            .remove(number) -> {
                if list.isValid(number) {
                    let task = list.remove(number)
                    print("Removed \"{task.title}\".")
                } else {
                    print("There's no task {number}.")
                }
            }
            .help -> printHelp()
            .quit -> { break }
            .unknown(text) -> print("Sorry, I don't understand \"{text}\". Type \"help\" to see the commands.")
        }
        saveTasks(list.tasks)
    }
    print("Bye!")
}
```

Check the whole folder:

```sh
tessel check todo
```

```text
ok: 4 files checked, no errors
```

## Trying it out

Put a whole session in a file, `session.txt`, one command per line:

```text
help
add Buy milk
add Water the plants
add Call grandma
list
done 2
remove 1
list
done 7
fly to the moon
quit
```

and feed it to the program with `<`, which sends a file to the program's
input as if you'd typed it:

```sh
tessel run todo < session.txt
```

```text
Your to-do list. Type "help" to see the commands.
Commands:
  add <title>   add a task
  list          show all tasks
  done <n>      mark task n as done
  remove <n>    remove task n
  quit          leave the program
Added "Buy milk".
Added "Water the plants".
Added "Call grandma".
1. [ ] Buy milk
2. [ ] Water the plants
3. [ ] Call grandma
Well done!
Removed "Buy milk".
1. [x] Water the plants
2. [ ] Call grandma
There's no task 7.
Sorry, I don't understand "fly to the moon". Type "help" to see the commands.
Bye!
```

Now start it again, and ask for the list:

```sh
echo list | tessel run todo
```

```text
Your to-do list. Type "help" to see the commands.
1. [x] Water the plants
2. [ ] Call grandma
Bye!
```

The tasks survived. A saved session like `session.txt` is also a simple
**test**: after changing the program, run it again and check that the
output is still right.

To use your to-do list without typing `tessel run` each time, build it into
a program of its own with `tessel build todo -o todo-app`, and run
`./todo-app`.

## Common mistakes

### `break` in a match arm, without braces

An arm without braces must be a
value, and `break` isn't one:

```tessel
            .quit -> break
```

```text
error: `break` in a match arm goes in braces
  --> todo/main.tsl:32:22
   |
32 |             .quit -> break
   |                      ^^^^^
   |
   = help: write `-> { break }`
```

Put it in braces, as the help says: `.quit -> { break }`. The same goes for
assignments.

### Forgetting a command in the `match`

Add a case to `Command` (or delete
an arm) and `tessel check` tells you which one isn't handled:

```text
error: this `match` doesn't handle `.help`
 --> todo/main.tsl:9:15
  |
9 |         match command {
  |               ^^^^^^^
  |
  = help: add arms for them, or `_ -> …` to handle the rest
```

This is one of the best reasons to use an enum for the commands: when you
add a new one later, Tessel points at every place that needs to handle it.
Resist adding a `_ -> …` arm here, since it would hide exactly those
reminders.

### Off by one with task numbers

The user's task 1 is at index 0. Forget
the `- 1` in `markDone`, and `done 1` marks the *second* task, and `done 3`
on a list of three stops the program with "index 3 is out of range for a
list of 3 items". Keeping the conversion in one place (the `TodoList`
methods) means you only have to get it right once.

## Ideas to extend it

**1. Undo "done".** Add an `undone <n>` command that marks a task as not
done. (You'll need a new case, a line in `parseCommand`, a method, and an
arm in `main`. `tessel check` will remind you of the arm.)

**2. Clear finished tasks.** Add a `clear` command that removes all done
tasks and says how many it removed.

<details>
<summary>Solution</summary>

Add `clear` to `Command`, and `"clear" -> Command.clear` to the `match` in
`parseCommand`. In `TodoList`:

```tessel
    // Removes the done tasks, and returns how many there were.
    fn clearDone() -> Int {
        let before = tasks.count
        tasks.removeAll(where: { t in t.done })
        before - tasks.count
    }
```

And in `main`:

```tessel
            .clear -> {
                let count = list.clearDone()
                print("Removed {count} done tasks.")
            }
```

</details>

**3. Edit a title.** Add `edit <n> <new title>`, like `edit 1 Water the
roses`. The command carries two values: `edit(number: Int, title: String)`.

<details>
<summary>Solution</summary>

In `commands.tsl`, add the case `edit(number: Int, title: String)`, the arm
`"edit" -> editCommand(line, rest: rest)`, and:

```tessel
// "edit 2 Buy oat milk": a number, then the new title.
fn editCommand(_ line: String, rest: String) -> Command {
    let words = rest.words()
    let title = words.dropFirst().joined(separator: " ")
    if let number = Int(words.first ?? "") {
        if !title.isEmpty {
            return Command.edit(number: number, title: title)
        }
    }
    Command.unknown(text: line)
}
```

In `TodoList`:

```tessel
    fn rename(_ number: Int, title: String) {
        tasks[number - 1].title = title
    }
```

And in `main`:

```tessel
            .edit(number, title) -> {
                if list.isValid(number) {
                    list.rename(number, title: title)
                    print("Renamed task {number}.")
                } else {
                    print("There's no task {number}.")
                }
            }
```

With both extensions, this session:

```text
add Buy milk
add Water the plants
add Call grandma
done 1
done 3
clear
edit 1 Water the roses
edit 1
list
quit
```

prints:

```text
Your to-do list. Type "help" to see the commands.
Added "Buy milk".
Added "Water the plants".
Added "Call grandma".
Well done!
Well done!
Removed 2 done tasks.
Renamed task 1.
Sorry, I don't understand "edit 1". Type "help" to see the commands.
1. [ ] Water the roses
Bye!
```

</details>

**4. Priorities.** Give `Task` a `priority` field using an enum
(`low`, `normal`, `high`), with a default of `normal`. Because the field has
a default, lists saved before the change still load (see
[Changing your types later](/guides/json/#changing-your-types-later)). Let
`add! Pay the rent` add a high-priority task, and show high-priority tasks
with a `!`.

**5. Several lists.** Let the user keep separate lists (`home`, `work`),
each in its own file in the app's folder. A `use work` command switches
lists; `lists` shows them all (`listFolder` will help).

## Summary

- Plan first: nouns become structs, a fixed set of actions becomes an enum.
- A `while let line = readLine()` loop reads commands until the input ends;
  `break` leaves it early.
- **Parse** input into an enum right away. The rest of the program then
  deals with checked values, and `match` makes sure every command is
  handled.
- Keep data that changes in a struct with methods, and check user input
  (like task numbers) before using it as an index.
- Save after every change, with JSON, in `appDataFolder`.
- Split a growing program into files by topic, and run the folder.
- A file of input fed with `<` is a quick, repeatable test.

**Next:** [19. Your first app with a window](/learn/19-first-app/)
