---
title: 23. Talking over the network
description: Connect two programs with TCP, send packets with UDP, find other computers on the local network, and serve a web page from your own program.
---

So far your programs have talked to web servers: `fetch` and `request`
asked, and a server somewhere answered. In this lesson your program is the
one being talked to, and it talks to programs like itself, not only to web
servers. That's how chat apps, multiplayer games, printers, smart lights
and file-sharing programs work: with **sockets**.

In this lesson you'll learn:

- what addresses, ports and sockets are
- how `connectTCP` opens a connection to another program, and how
  `listenTCP` accepts connections from others
- why TCP gives you a stream of bytes, and how to cut it into messages
- how `openUDP` and `sendUDP` send single packets, and how a multicast
  group lets programs find each other
- how `serveHTTP` makes your program a web server a browser can open

At the end, you'll build a chat for the local network: one copy hosts,
the others join it.

## Addresses, ports and sockets

Every computer on a network has an **address**, like `192.168.1.20` on
your home network, or `2606:4700::1111` (the newer, longer kind, IPv6).
Your own computer is always reachable as `127.0.0.1` (or `localhost`),
which is handy for trying things out: a program can talk to another
program on the same machine.

An address finds the computer; a **port** finds the program on it. A port
is a number from 1 to 65535. A program that wants to be talked to
*listens* on a port, and others *connect* to that address and port. Web
servers listen on port 80 (and 443 for `https`); a port below 1024 needs
administrator rights to listen on, so programs of your own use higher
ones, like 7070 or 8080. Port 0 means "any free one".

A **socket** is one end of a conversation: what your program sends to and
reads from. In Tessel a socket is an `Int` id, like a timer's or a
WebSocket's, and what arrives on it comes to a block, like everything that
happens later.

## A connection: TCP

**TCP** is a connection between two programs: once made, bytes sent at one
end come out at the other, all of them, in order. That's what the web,
email and most of the internet run on. Here is a program that listens, and
tells the time to whoever connects:

```tessel
fn main() {
    let server = listenTCP(port: 7070, onConnection: { id, peer in
        print("{peer} asked for the time")
        sendTCP(id, data: Data(text: formatDate(now()) + "\n"))
        closeTCP(id)
    }, onData: { id, data in }) { id, reason in }
    if server == -1 {
        print("can't listen: port 7070 is taken")
        return
    }
    print("Ask for the time: nc localhost 7070")
}
```

`listenTCP` takes the port and three blocks: one for each new connection
(with the connection's id and who it is, as `"address:port"`), one for
data that arrives on a connection, and one for a connection ending. It
returns the listener's id, or -1 if it couldn't listen (someone else has
the port). Run it: it prints its line and keeps running, because a program
with an open socket isn't done, just as one with a running timer isn't.

Now a program that asks. In another terminal:

```tessel
fn main() {
    var socket = -1
    socket = connectTCP(host: "localhost", port: 7070, onConnect: {
        print("connected to {tcpPeer(socket)}")
    }, onData: { data in
        print("the server says: {data.text ?? ""}", terminator: "")
    }) { reason in
        print("connection ended: {reason}")
    }
}
```

```text
connected to 127.0.0.1:7070
the server says: 2026-11-06 18:53
connection ended: closed
```

`connectTCP` returns right away with the connection's id; making the
connection takes time (a name to look up, a computer to reach), so a
block, `onConnect`, says when it's ready. `onData` gets what arrives;
`onClose` runs once at the end, with `"closed"` when a side hung up, or
why it failed: `"connection refused"` (nothing listens there), `"timed
out"`, `"can't resolve host"`. Try it with the server stopped.

Both programs could have been one: a program can listen and connect to
itself, which is how you'll test the chat at the end.

### A stream, not messages

Something to know before writing a protocol of your own: TCP carries
**bytes, not messages**. If a program sends `"hello"` and then `"bye"`, the
other side may get `"hellobye"` in one piece, or `"hel"` and `"lobye"`,
or anything else that adds up. Networks cut and join as they please. So
the two sides must agree on where a message ends. The simplest way is a
line break at the end of each message: keep whatever came after the last
line break, and wait for the rest.

```tessel
var partial = ""

fn gotData(_ data: Data) {
    let text = partial + (data.text ?? "")
    let lines = text.split("\n")
    partial = lines.last ?? ""          // an unfinished line, or ""
    for line in lines.dropLast() {
        handle(line)
    }
}
```

The other common way is to send a number in front of each message saying
how long it is. Either way: never assume one `onData` is one message.

## Packets: UDP

Sometimes a connection is more than you need. A game sends where the
player is thirty times a second: if one update is lost, the next one is
along right away, and waiting to resend the old one would only make
things worse. For that there's **UDP**: a socket sends single packets
to any address and port, and receives any sent to it. Nothing is
connected and nothing is promised; a packet may be lost, or arrive twice,
or out of order. In return it's quick, and one packet can go to many
computers at once.

Two UDP sockets in one program, playing ping-pong:

```tessel
fn main() {
    var a = -1
    var b = -1
    a = openUDP { data, host, port in
        print("a got \"{data.text ?? ""}\" from {host}:{port}")
        closeUDP(a)
        closeUDP(b)
    }
    b = openUDP { data, host, port in
        print("b got \"{data.text ?? ""}\" from {host}:{port}")
        sendUDP(b, data: Data(text: "pong"), host: host, port: port)
    }
    sendUDP(a, data: Data(text: "ping"), host: "127.0.0.1", port: udpPort(b))
}
```

```text
b got "ping" from 127.0.0.1:55123
a got "pong" from 127.0.0.1:55124
```

`openUDP` takes a port (none here: any free one, which `udpPort` tells)
and a block that gets each packet with who sent it, so the socket can
answer. The program ends once both sockets are closed.

### Finding each other

How does a chat know which address to join? You could type it in, and the
chat below does. But programs on the same network can also find each
other, by all listening to a **multicast group**: an address (from
`224.0.0.0` to `239.255.255.255`) that isn't a computer but a club. A
packet sent to the group reaches every member. This program joins one,
says its name every two seconds, and lists the others it hears:

```tessel
fn main() {
    let group = "239.255.42.1"
    let port = 42424
    let me = "player-{random(1000..9999)}"
    var seen: [String: Float] = [:]
    let socket = openUDP(port: port) { data, host, from in
        let name = data.text ?? ""
        if name != me && seen[name] == nil {
            print("found {name} at {host}")
        }
        seen[name] = now()
    }
    if socket == -1 || !joinMulticast(socket, group: group) {
        print("no network to look on")
        return
    }
    print("I am {me}, listening on port {udpPort(socket)}")
    every(seconds: 2) {
        sendUDP(socket, data: Data(text: me), host: group, port: port)
    }
}
```

Start it twice (on one computer, or on two that share a Wi-Fi network):

```text
I am player-4718, listening on port 42424
found player-3829 at 192.168.1.128
```

Two things made this work: `joinMulticast` made the socket a member of the
group, and every copy listens on the *same* port (UDP sockets share their
ports), so a packet to the group's address and that port finds them all.

## Your program as a web server

The web's protocol, HTTP, is a conversation over TCP: the browser sends a
request (`GET /page`), the server answers with a status and a body. You
could write that over `listenTCP`, but Tessel reads and writes HTTP for
you: `serveHTTP` gets each request as a `ServerRequest`, and
`respondHTTP` answers it.

```tessel
fn main() {
    var hits = 0
    let server = serveHTTP(port: 8080) { r in
        hits += 1
        if r.path == "/" {
            respondHTTP(r.id, headers: ["Content-Type": "text/html"], body: Data(text: "<h1>Hit {hits}</h1>"))
        } else {
            respondHTTP(r.id, status: 404, body: Data(text: "Nothing at {r.path}"))
        }
    }
    if server == -1 {
        print("port 8080 is taken")
        return
    }
    print("open http://127.0.0.1:{tcpPort(server)}/")
}
```

Run it and open the address in a browser: each reload counts a hit. The
request `r` has the method (`"GET"`, `"POST"`), the path, the query
(what's after `?`), the headers, the body, and who asked. `respondHTTP`
takes the request's id, a status (200 unless you say), headers and a
body; it can be called later, from another block, when the answer takes
time.

By default the server listens on `127.0.0.1`: only your own computer can
open it. `host: "0.0.0.0"` lets the whole local network, which makes a
program a remote control for itself: a page with buttons, opened on a
phone. (The first time, macOS and Windows ask whether to let the app
accept connections: that's their firewall. Linux usually doesn't ask.)

## Worked example: a chat for the local network

Let's put TCP in an app: a chat where one copy hosts, and others join by
typing the host's address. The host is a `listenTCP` server that passes
everything it gets to every connection; each copy, the host's own
included, is a `connectTCP` client of it. That way the host needs no
special case for its own messages.

```tessel
let port = 5050

app Chat(width: 440, height: 360) {
    state name = "Ada"
    state address = "127.0.0.1"
    state server = -1            // the listener, while hosting
    state clients: [Int] = []    // the host's connections
    state connection = -1        // this copy's own connection to the host
    state messages: [String] = []
    state draft = ""

    VStack(spacing: 8, alignment: .leading) {
        HStack {
            TextField("Your name", text: name)
            Button("Host") { host() }.disabled(server != -1)
            TextField("Address", text: address)
            Button("Join") { join() }.disabled(connection != -1)
        }
        for message in messages {
            Text(message)
        }
        Spacer()
        TextField("Say something", text: draft).onSubmit { send() }
    }
    .padding(12)

    fn host() {
        server = listenTCP(port: port, onConnection: { id, peer in
            clients.append(id)
        }, onData: { id, data in
            // Whatever one client says goes to every client.
            for client in clients {
                sendTCP(client, data: data)
            }
        }) { id, reason in
            clients.removeAll { client in client == id }
        }
        if server == -1 {
            messages.append("Can't host: port {port} is in use")
            return
        }
        messages.append("Hosting on port {port}: others join at {localAddresses().first ?? "127.0.0.1"}")
        address = "127.0.0.1"
        join()
    }

    fn join() {
        connection = connectTCP(host: address, port: port, onConnect: {
            messages.append("Connected to {address}")
        }, onData: { data in
            for line in (data.text ?? "").lines() {
                messages.append(line)
            }
        }) { reason in
            messages.append("Disconnected: {reason}")
            connection = -1
        }
    }

    fn send() {
        if connection != -1 && draft != "" {
            sendTCP(connection, data: Data(text: "{name}: {draft}\n"))
            draft = ""
        }
    }
}
```

Run it, click Host, and type something: it comes back through the server
and appears. Run a second copy (on this computer, or another on the same
network), type the address the first one printed, click Join, and the two
talk. `localAddresses()` gives this computer's addresses on the network,
which is what the others need; `127.0.0.1` only works from the same
machine.

Each message ends with `\n`, and the reader splits on lines, as the
section on streams said. The chat has no names list and no history for
latecomers; those are the exercises.

## Checking it without a window

A [headless run](/ui/testing/) can host and join itself:

```sh
TESSEL_SCRIPT='click "Host"; wait 0.5; click field 3; type hi there; key enter; wait 0.5; tree' tessel run chat.tsl
```

```text
Root
  View
    VStack
      HStack
        TextField "Your name" = "Ada"
        Button "Host" disabled
        TextField "Address" = "127.0.0.1"
        Button "Join" disabled
      Text "Hosting on port 5050: others join at 192.168.1.128"
      Text "Connected to 127.0.0.1"
      Text "Ada: hi there"
      Spacer
      TextField "Say something" = ""
```

`wait` lets what's arriving on open sockets come in for a moment, as it
does for WebSockets, so the message has come back by the time `tree`
prints.

## Common mistakes

### Treating each `onData` as one message

A message may arrive in pieces, and several at once. Cut the stream
yourself: by lines, or by a length sent in front.

### Forgetting the program keeps running

A `fn main` program with an open listener never ends by itself. That's
what a server wants; a client should `closeTCP` when it's done, or call
`exit`.

### Listening on a port someone else has

`listenTCP` and `serveHTTP` return -1 when the port is taken (or below
1024). Check for it, and let the user pick another, or use port 0 and
tell them what `tcpPort` gave.

### Sending to `127.0.0.1` from another computer

`127.0.0.1` always means "this computer". For others to reach you, tell
them an address from `localAddresses()`.

## Exercises

**1. Who's here.** Give the chat a `/who` message: when the host gets a
line whose text is `/who` (after the name, so `"Ada: /who"`), it sends
back the number of connections (`"3 here\n"`) to the one who asked,
instead of passing the line on.

<details>
<summary>Solution</summary>

In `host()`'s `onData` block:

```tessel
        }, onData: { id, data in
            if (data.text ?? "").hasSuffix(": /who\n") {
                sendTCP(id, data: Data(text: "{clients.count} here\n"))
                return
            }
            for client in clients {
                sendTCP(client, data: data)
            }
        }) { id, reason in
```

</details>

**2. Latecomers.** Keep the last 20 lines on the host, and send them to
each new connection as it arrives, so someone who joins late sees what was
said.

<details>
<summary>Solution</summary>

Add `state history: [String] = []` to the app, and in `host()`:

```tessel
        server = listenTCP(port: port, onConnection: { id, peer in
            clients.append(id)
            if !history.isEmpty {
                sendTCP(id, data: Data(text: history.joined("\n") + "\n"))
            }
        }, onData: { id, data in
            for line in (data.text ?? "").lines() {
                history.append(line)
            }
            history = history.suffix(20)
            for client in clients {
                sendTCP(client, data: data)
            }
        }) { id, reason in
```

</details>

**3. A counter you can read from anywhere.** Write a `fn main` program
with a number that `every` second goes up by one, and a `serveHTTP`
server on port 8080 whose `/` page shows the number and whose `/reset`
(any method) sets it back to zero. Open it in a browser, and from a
terminal: `curl -X POST http://127.0.0.1:8080/reset`.

<details>
<summary>Solution</summary>

```tessel
fn main() {
    var count = 0
    every(seconds: 1) { count += 1 }
    serveHTTP(port: 8080) { r in
        match r.path {
            "/" -> respondHTTP(r.id, headers: ["Content-Type": "text/html"], body: Data(text: "<h1>{count}</h1>"))
            "/reset" -> {
                count = 0
                respondHTTP(r.id, status: 204)
            }
            _ -> respondHTTP(r.id, status: 404)
        }
    }
}
```

</details>

## Summary

- An **address** finds a computer, a **port** finds a program on it;
  `127.0.0.1` is your own computer. A **socket** is one end of a
  conversation, an `Int` id in Tessel.
- **`connectTCP(host:port:onConnect:onData:onClose:)`** opens a
  connection; **`listenTCP(port:onConnection:onData:onClose:)`** accepts
  them. **`sendTCP`** sends, **`closeTCP`** hangs up, **`tcpPort`** and
  **`tcpPeer`** say where things are.
- TCP is a **stream of bytes**: cut it into messages yourself (by lines,
  or by a length sent first).
- **`openUDP(port:onPacket:)`** and **`sendUDP`** send single packets, with
  no connection and no promises; **`joinMulticast`** makes a socket hear a
  group, which is how programs find each other.
- **`serveHTTP(port:onRequest:)`** makes the program a web server;
  **`respondHTTP(id, status:headers:body:)`** answers a `ServerRequest`.
- A program with an open socket keeps running; -1 from any of these means
  it couldn't start (the port is taken).
- For more, see [Sockets and servers](/guides/networking/) and the
  [reference](/reference/stdlib/#sockets).

**Next:** [24. Where to go next](/learn/21-next-steps/)
