---
title: Sockets and servers
description: "TCP connections and servers, UDP packets, multicast discovery, an HTTP server of your own, and what to know about firewalls, ports and testing."
---

[`fetch`, `request`, `download`](/ui/async/#downloading-text-with-fetch) and
[WebSockets](/ui/async/#websockets) talk to web servers. For everything
else on a network, Tessel has sockets: a **TCP** connection is a two-way
stream of bytes to one other program, a **UDP** socket sends and receives
single packets (to anyone, including a whole group at once), and
[`serveHTTP`](/reference/stdlib/#servehttp) makes your program a web server.
With them a program can talk to a device on the local network, find other
copies of itself, take part in a peer-to-peer protocol, or be driven from a
browser.

Everything here works the same in an app and in a program with only
`fn main()`, which keeps running while a socket is open. Blocks run on
the UI thread, so they can change `state`.

## A TCP connection

[`connectTCP`](/reference/stdlib/#connecttcp) opens a connection and
returns its id at once; the connection is made in the background, and three
blocks tell you what happens:

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

- `onConnect` runs once the connection is made. `host` can be a name or an
  IP address (IPv4 or IPv6); a name is looked up in the background, and
  each of its addresses tried in turn.
- `onData` runs with each piece of bytes that arrives, as
  [`Data`](/reference/stdlib/#data).
- `onClose` runs exactly once, when the connection ends: `"closed"` if you
  or the other end hung up, or why it failed: `"connection refused"`,
  `"timed out"` (after `timeout:` seconds, 10 unless you say), `"can't
  resolve host"`…

[`sendTCP(id, data:)`](/reference/stdlib/#sendtcp) sends bytes (queued
until the connection is ready, if need be), and
[`closeTCP(id)`](/reference/stdlib/#closetcp) hangs up after sending what's
queued.

### A server

[`listenTCP`](/reference/stdlib/#listentcp) waits for connections. Each
one gets an id of its own, which you send to and close like a connection
you made yourself. This server tells the time to whoever connects:

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

Run it, then the client above in another terminal (or `nc localhost
7070`), and the client prints `the server says: 2026-11-06 18:53` and
`connection ended: closed`.

- `port: 0` takes any free port; [`tcpPort(server)`](/reference/stdlib/#tcpport)
  says which. Ports below 1024 need administrator rights.
- `host:` is which of this computer's addresses to listen on: all of them
  (`"0.0.0.0"`, the default), only this computer (`"127.0.0.1"`: nothing
  from the network can reach it), or all with IPv6 too (`"::"`).
- Closing the listener closes its connections as well.

:::note[Firewalls]
The first time an app listens on `"0.0.0.0"`, macOS asks whether to allow
incoming connections (every time, for an app that isn't signed), and
Windows shows its firewall dialog. Listening on `"127.0.0.1"` asks nothing.
:::

### Streams, not messages

TCP carries bytes, not messages: what one side sends in one `sendTCP` may
arrive in several `onData` pieces, and two sends may arrive in one. A
protocol needs a way to tell where a message ends: a line break, a length
in front of each message (what most binary protocols do), or a fixed size.
A server that answers each line, keeping whatever came after the last line
break until the rest of that line arrives:

```tessel
fn main() {
    var server = -1
    var partial: [Int: String] = [:]
    server = listenTCP(port: 0, host: "127.0.0.1", onConnection: { id, peer in
        print("server: {peer} connected")
        partial[id] = ""
    }, onData: { id, data in
        let text = (partial[id] ?? "") + (data.text ?? "")
        let lines = text.split("\n")
        partial[id] = lines.last ?? ""
        for line in lines.dropLast() {
            sendTCP(id, data: Data(text: "You said: {line}\n"))
        }
    }) { id, reason in
        print("server: {id} left ({reason})")
        partial[id] = nil
        closeTCP(server)
    }

    var client = -1
    client = connectTCP(host: "127.0.0.1", port: tcpPort(server), onConnect: {
        sendTCP(client, data: Data(text: "hello\nbye\n"))
    }, onData: { data in
        print(data.text ?? "", terminator: "")
        if (data.text ?? "").contains("bye") {
            closeTCP(client)
        }
    }) { reason in
        print("client: {reason}")
    }
}
```

```text
server: 127.0.0.1:63904 connected
You said: hello
You said: bye
client: closed
server: 3 left (closed)
```

(The server and the client are in one program here, which is how the
tests in Tessel's own repository work too.) For a binary protocol, read
numbers out of the `Data` with
[`int32(at:)` and its kin](/reference/stdlib/#numbers-in-bytes), and build
messages with [`Data`'s builders](/reference/stdlib/#building-bytes).

### Sending a lot: backpressure

`sendTCP` returns at once and queues what it's given; the bytes go out as
fast as the other end takes them. A sender that queues faster than that
holds more and more in memory, so when sending a big file, look at
[`tcpQueued`](/reference/stdlib/#tcpqueued) and wait before queuing more:

```tessel
fn pump() {
    while sent < total && tcpQueued(socket) < 4 * 1048576 {
        sendTCP(socket, data: readData(path, from: sent, count: 65536) ?? Data())
        sent += 65536
    }
    if sent < total {
        after(seconds: 0.01) { pump() }
    }
}
```

On the receiving side, [`pauseTCP(id, paused: true)`](/reference/stdlib/#pausetcp)
stops `onData` until you resume (the sender stops being able to send once
the system's buffers fill up), for rate limits or for not taking more than
you can handle. Tessel also stops reading a connection by itself while a
few megabytes wait for a block that hasn't got to them yet, so a fast peer
can't fill memory.

### Many connections

One thread serves every socket (with the system's kqueue, IOCP or epoll),
so a program can hold thousands of connections at once: 5,000 open
connections to one listener in half a second, on a laptop. Two things to
know:

- A burst of more new connections than the system's queue of waiting ones
  (128 on macOS unless `kern.ipc.somaxconn` is raised; thousands on Linux)
  makes some of them retry and wait; a client that opens many connections
  should open them in batches.
- The system limits open files (sockets count); Tessel raises the limit
  for its process where it can.

## UDP

[`openUDP`](/reference/stdlib/#openudp) makes a socket that sends and
receives single packets, up to 64 KB each, to and from anyone. Nothing is
connected, nothing is promised: a packet may be lost, or come twice or out
of order. That's what makes UDP right for things said often and cheaply
(game state, "is anyone there?", DNS, BitTorrent's tracker and DHT
protocols).

```tessel
fn main() {
    var socket = -1
    socket = openUDP(port: 5000) { data, host, port in
        print("{host}:{port} says {data.text ?? "?"}")
        sendUDP(socket, data: Data(text: "got it"), host: host, port: port)
    }
}
```

`onPacket` gets each packet with its sender's address and port, so the
socket can answer. [`sendUDP`](/reference/stdlib/#sendudp) sends one to a
host and port; `"255.255.255.255"` reaches every computer on the local
network at once.

### Finding others: multicast

A multicast group is an address that any number of programs can listen
to: a packet sent to it reaches them all. It's how programs find each
other on a local network (AirPlay, printers, BitTorrent's local peer
discovery, SSDP). This program announces itself to a group every two
seconds and lists who else it hears:

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

Two copies, started on the same computer:

```text
I am player-4718, listening on port 42424
found player-3829 at 192.168.1.128
```

- [`joinMulticast`](/reference/stdlib/#joinmulticast) subscribes the
  socket to the group (`224.0.0.0` to `239.255.255.255`; pick one in
  `239.255.0.0/16` for your own program). To be heard, send to the group's
  address and the port everyone listens on.
- Several programs on one computer can listen on the same port this way:
  a UDP socket shares its port with others that share theirs.
- Without a network (no Wi-Fi, no cable), joining fails, and the program
  above says so.

## An HTTP server

[`serveHTTP`](/reference/stdlib/#servehttp) makes your program a web
server: a block gets each request as a
[`ServerRequest`](/reference/stdlib/#serverrequest) (its method, path,
query, headers, body, and who sent it), and
[`respondHTTP`](/reference/stdlib/#respondhttp) answers it, at once or
later. This one is a web remote: open the address it prints in a browser
on any device on the network, and the buttons change a value in the
program:

```tessel
fn main() {
    var volume = 50
    let page = [
        "<!doctype html><title>Remote</title>",
        "<h1>Volume: <span id=v>?</span></h1>",
        "<button onclick=\"change(-10)\">-</button> <button onclick=\"change(10)\">+</button>",
        "<script>",
        "async function show() \{ document.getElementById('v').textContent = await (await fetch('/volume')).text() \}",
        "async function change(d) \{ await fetch('/volume?by=' + d, \{method: 'POST'\}); show() \}",
        "show()",
        "</script>",
    ].joined("\n")
    let server = serveHTTP(port: 8585, host: "0.0.0.0") { r in
        match r.path {
            "/" -> respondHTTP(r.id, headers: ["Content-Type": "text/html"], body: Data(text: page))
            "/volume" -> {
                if r.method == "POST" {
                    let by = Int(r.query.replace("by=", with: "")) ?? 0
                    volume = min(max(volume + by, 0), 100)
                    print("{r.remote} set the volume to {volume}")
                    respondHTTP(r.id, status: 204)
                } else {
                    respondHTTP(r.id, body: Data(text: "{volume}"))
                }
            }
            _ -> respondHTTP(r.id, status: 404, body: Data(text: "not here"))
        }
    }
    if server == -1 {
        print("can't serve: port 8585 is taken")
        return
    }
    for address in localAddresses() {
        let host = if address.contains(":") { "[{address}]" } else { address }
        print("open http://{host}:{tcpPort(server)}/")
    }
}
```

```text
open http://192.168.1.128:8585/
127.0.0.1:64323 set the volume to 60
```

- `host:` is `"127.0.0.1"` unless you say: only this computer can reach the
  server. `"0.0.0.0"` opens it to the network (and brings the firewall
  question above).
- `Content-Length` is added to answers for you; give a `Content-Type` so
  browsers know what they're getting. An answer can come from a later
  block: after a `request` of your own to another server, a timer, or
  `background` work.
- The server speaks HTTP/1.1, with keep-alive connections and request
  bodies of a known length or in chunks. It doesn't speak HTTPS: for a
  server on the internet, put it behind one that does.
- [`localAddresses()`](/reference/stdlib/#localaddresses) are the
  addresses others can reach this computer at;
  [`resolveHost`](/reference/stdlib/#resolvehost) turns a name into
  addresses.

## In an app

Sockets in an `app` work the same; keep their ids in `state`, and close
them when done. The course's [LAN chat](/learn/23-networking/#worked-example-a-chat-for-the-local-network)
is an app that hosts a server and joins it.

## Testing

A program with sockets tests like any other: the ones in Tessel's own
`tests/run/` start a listener on port 0 and connect to it from the same
program, so they need no network and no free port of a known number. In an
app's [headless script](/ui/testing/), `wait` lets what's arriving on open
sockets come in for a moment (half a second, or as long as the wait), as
it does for open WebSockets; use `waitfor` to wait for what a message
brings about.

A program with only `fn main` keeps running while a listener, a
connection or a UDP socket is open: close them (or call `exit`) to let it
end.
