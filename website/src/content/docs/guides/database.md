---
title: Saving data in a database
description: "Keep an app's data in a SQLite database: tables, adding and changing rows with values for `?`s, finding rows, transactions, and errors."
---

When an app's data grows past what's comfortable to load and save in one
piece (thousands of records, or data you want to search and sort), keep it
in a database. Tessel has [SQLite](https://sqlite.org) built in: a whole
database in one file, read and changed with SQL, which works the same on
macOS and Windows and needs nothing installed.

For smaller data, like settings or a document, [saving it as JSON](/guides/json/)
is simpler.

## Opening a database

```tessel
fn main() {
    let db = try openDatabase("survey.db")
    try db.execute("CREATE TABLE IF NOT EXISTS parcels (id INTEGER PRIMARY KEY, name TEXT NOT NULL, area REAL NOT NULL, note TEXT)")
    try db.execute("INSERT INTO parcels (name, area, note) VALUES (?, ?, ?)", ["Lot 12", 1250.5, nil])
    for row in try db.query("SELECT name, area FROM parcels WHERE area > ? ORDER BY area", [1000]) {
        print("{row.text("name")}: {row.float("area")} m²")
    }
}
```

- [`openDatabase`](/reference/stdlib/#opendatabase) opens the file, or
  creates it; `":memory:"` is a database that lasts only while the program
  runs. In an app, keep the file in
  [`appDataFolder`](/reference/stdlib/#appdatafolder).
- `execute` runs SQL that changes something (`CREATE TABLE`, `INSERT`,
  `UPDATE`, `DELETE`) and returns how many rows it changed. `query` runs a
  `SELECT` and returns its rows.
- Both return a `Result`, as reading a file does: `try` passes a failure on
  (with SQLite's reason, like `no such table: parcels`), or handle it with
  `match`. See [Errors](/language/errors/).

## Values for `?`

Write a `?` in the SQL for each value, and give the values in a list, in
order. An `Int`, `Float`, `String`, `Bool` (stored as 1 or 0) or `Data` is
given as it is, an optional one too, and `nil` stores null:

```tessel
try db.execute("UPDATE parcels SET area = ?, note = ? WHERE name = ?", [area, note, name])
```

Never build SQL by putting values into its text (`"… WHERE name = '{name}'"`):
a name with a quote in it would break the SQL, or change what it does. With
`?`, SQLite keeps values apart from the SQL.

## Reading rows

`query` gives a list of rows. A row's `int`, `float`, `text`, `bool` and
`data` give a column by its name (as the query names it, so give
calculated columns a name with `AS`), and `isNull` tells whether it's
null:

```tessel
let totals = try db.query("SELECT count(*) AS n, sum(area) AS total FROM parcels")
print("{totals[0].int("n")} parcels, {totals[0].float("total")} m²")
```

A column that holds another kind of value is converted the way SQLite
converts it (text `"12"` as an `Int` is 12), and null reads as 0, `""` or
`false`. Asking for a column the row doesn't have stops the program with
an error, since it's a mistake in the program. To turn rows into your own
structs, map them:

```tessel
struct Parcel {
    name: String
    area: Float
}

let parcels = (try db.query("SELECT name, area FROM parcels")).map { r in Parcel(name: r.text("name"), area: r.float("area")) }
```

## Several changes as one

SQL without values can be several statements, separated by `;`. A
transaction makes several changes happen together, or not at all:

```tessel
try db.execute("BEGIN")
try db.execute("UPDATE accounts SET balance = balance - ? WHERE id = ?", [amount, from])
try db.execute("UPDATE accounts SET balance = balance + ? WHERE id = ?", [amount, to])
try db.execute("COMMIT")
```

If something fails in between, `ROLLBACK` undoes what was done since
`BEGIN`. Many changes inside one transaction are also much faster than
the same changes one by one.

## In an app

Open the database once, when the app starts, and keep it in `state`. Here
a list of parcels is read when the window appears, and again after each
one is added:

```tessel
struct Parcel {
    id: Int
    name: String
    area: Float
}

/// The app's database, with its table made the first time.
fn openParcels() -> Result<Database> {
    let db = try openDatabase(joinPath(appDataFolder("Parcels"), "parcels.db"))
    try db.execute("CREATE TABLE IF NOT EXISTS parcels (id INTEGER PRIMARY KEY, name TEXT NOT NULL, area REAL NOT NULL)")
    Result.ok(value: db)
}

fn loadParcels(_ db: Database) -> [Parcel] {
    let rows = db.query("SELECT id, name, area FROM parcels ORDER BY name").valueOr([])
    return rows.map { r in Parcel(id: r.int("id"), name: r.text("name"), area: r.float("area")) }
}

app Parcels(width: 420, height: 300) {
    state db = openParcels().value()
    state parcels: [Parcel] = []
    state name = ""
    state area = ""
    state status = ""

    VStack(alignment: .leading) {
        HStack {
            TextField("Name", text: name)
            TextField("Area (m²)", text: area)
            Button("Add") {
                if let db = db {
                    match db.execute("INSERT INTO parcels (name, area) VALUES (?, ?)", [name, parseFloat(area).valueOr(0)]) {
                        .ok(_) -> {
                            parcels = loadParcels(db)
                            name = ""
                            area = ""
                        }
                        .failure(e) -> {
                            status = e.message
                        }
                    }
                }
            }
        }
        for p in parcels {
            Text("{p.name}: {p.area} m²")
        }
        Text(status)
    }
    .padding(12)
    .onAppear {
        if let db = db {
            parcels = loadParcels(db)
        }
    }
}
```

The database is closed when the app ends; `db.close()` closes it sooner.

## Good to know

- One file holds the whole database. Copy it to back it up, but not while
  the app is changing it.
- `db.lastInsertId()` is the `id` (`rowid`) of the row the last `INSERT`
  added.
- SQLite's SQL is described at [sqlite.org/lang.html](https://sqlite.org/lang.html).
