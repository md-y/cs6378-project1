# CS 6378 P2P File Sharing Project

This document discusses the design of my P2P file sharing program. The program is written in Rust.

## Architecture

The P2P system uses a layered architecture built on top of TCP/IP. Specifically, these are the layers:

- TCP/IP Layers: These layers consist of the standard network access, internet, and transport layers from the TCP/IP network model. Because they are standard, the program leverages built-in OS APIs. These are the lowest levels.
- Session Layer: This layer is the lowest application layer. It handles socket connections to other nodes and provides services to higher layers for broadcasting to machines, connecting to specific machines, and handling connections/disconnections. It also ensures a valid network topology.
- Search Layer: This layer manages search requests and enforces hop-count rules. It also handles pathing through the network.
- File Layer: This layer handles file uploads/downloads between nodes.

To support these layers, we have a core data structure called `Config`.
This stores information about the machine that the process is running on, the `adj` matrix, $n$, and all the routing info. When the process starts, `Config` is built by reading from the configuration files. If it is not valid, the program fails to start, including if `adj` is not a connected graph.

## Modules

Each layer has its own module.file:

- `session.rs`
- `search.rs` (to be added later)
- `file.rs` (to be added later)

Beyond these modules, we have these utility modules that support the layers:

- `config.rs` (the core module for `Config` that manages deserialization and verification)
- `adj.rs` (`adj`-specific logic used by the `Config` module, mostly in charge of verifying `adj` is connected)
- `message.rs` (contains `Message` enum that specifies each message type)

We will go into more detail on these later.

Finally, `main.rs` is the entry point of the application. It does not do much other than creating the `Config` object and starting all layers.

### `config.rs` and `adj.rs`

All machine information is configured in `.toml` files. This makes it easy to configure the program for different environments. In addition, you can merge configs together. This means we can have `adj` and routing information in shared `.toml` files.

When we start the program, each argument is interpreted as a config file path. We then use the `toml` and `serde` crates to deserialize these files into unverified config and `adj` structs. These can be merged together into a final candidate config.

After merging, we verify if the config is correct. This includes (but is not limited to):

- Checking all required settings are present (e.g. node ID, `adj` section, `routes` section, etc.)
- Checking if `adj` is connected and is $n$ by $n$
- Checking if the node ID is less than $n$ (since node ID is 0-indexed)

`adj` has its own section that has two settings:

- $n$ (the total number of nodes)
- matrix (a 2D matrix representing `adj`. This is converted into a 1D vector after deserialization to save memory.)

`routes` is another section that specifies the domain and port of each machine. Each value is a map between the node ID and the authority string (domain + port). For example:

```toml
0 = "dc01.utdallas.edu:50890"
1 = "dc02.utdallas.edu:50890"
2 = "dc03.utdallas.edu:50890"
```

`main.rs` triggers the config deserialization and saves the verified struct as a singleton available to all layers.

### `session.rs`

When the session layer is initialized, is scans `adj` to determine which nodes it should send connection requests to. Since we only need one node to initiate a request in order to establish a bidirectional TCP channel, we need to decide on a total ordering. This is done using these rules:

- If $\operatorname{Adj}[i, j] = 1$ but $\operatorname{Adj}[j, i] \neq 1$, then node $i$ requests to connect to node $j$.
- If $\operatorname{Adj}[i, j] = 1$ and $\operatorname{Adj}[j, i] = 1$, then node $i$ requests to connect to node $j$ if and only if $i < j$.

To connect to a node, we send an `InitRequest` message. All messages are instances of the `Message` struct, and each has a body from the `MessageBody` enum. The messages are serialized using `bson`, and `serde` automatically differentiates each message body type using its `tag` trait.

I chose to use `bson` because I am most familiar with using JSON for network requests. However, we are sending files over the network for this project, and JSON does not support binary data, so it would add significant overhead. Therefore, `bson` is a good alternative since it supports binary data and automatically includes length information when serialized, allowing for easy detection of when messages end.

In addition to sending connection requests, the session layer also listens for any new TCP connections in a separate coroutine. It accepts any request and then listens for a `InitRequest` message. If the sender correctly sends a `InitRequest` message, the recipient node returns a `InitResponse` message. This is the handshake for all nodes in the application. If the sender fails to do this handshake, its request is ignored.

All successful connections are recorded in a shared hash map. This hash map is protected behind a mutex since connections are in separate coroutines.

Once all the required connections have been established (as determined by `adj`), the node is considered ready and the higher layers are enabled.

### `search.rs` and `file.rs`

These layers will be implemented in future parts. However, they will work broadly as such.

The search layer handles navigating the network topology. It accepts all non-init messages from the session layer. The message types include:

- `SearchRequest` (these messages include the sender and desired files, and each node that receives it along the way will forward it)
- `SearchResponse` (the response to `SearchRequest` containing file information and the node that possesses it)

The layer sends out `SearchRequest` messages as a broadcast along its edges if it does not know where the file is located. This means that it can receive many `SearchResponse` messages per `SearchRequest` if multiple nodes have the file. To account for this (and to accept random `SearchResponse` messages), we store the received file information in another mutex-protected hash map. We use this stored information for the file layer.

The file layer also handles specific message types:
 - `FileRequest` (message contains which file the sender wants and which parts of it)
 - `FileResponse` (chunk of the requested file, terminated by a message with the file hash)

When the use requests a file, the file layer uses the stored file information from the search layer to request the file-holder node(s). It then listens for file chunks until the file is complete. It then verifies the file is valid by comparing the combined result to the hash.

The file layer also records file information if forwards in its local file object.

For both of these layers, messages are each handled in their own coroutine. This allows each node to handle multiple requests at a time.

## Algorithms

This section discusses specific algorithms used in the program.
These are mainly ones that were specifically instructed to be mentioned.

### Adj Connectivity Check

Upon initialization, each node checks if `adj` is valid by ensuring it has the correct dimensions ($n$ by $n$) and if it is connected (i.e. it is a single connected component).

For this latter check, we do a simple breadth-first search. We keep track of each node we visit and once the queue of unvisited nodes is empty, we see if we visited $n$ nodes. We add nodes to the queue if they have not been visited already, and if either $\operatorname{Adj}[i, j]=1$ or $\operatorname{Adj}[j, i]=1$ since TCP connections are bidirectional, so the edges are undirected. We also ignore the diagonal ($i=j$).

Below is the code that does this check.

```rust
// self is the Adj struct
pub fn is_connected(&self) -> bool {
    let mut seen = HashSet::<u32>::new();
    let mut queue = Vec::<u32>::with_capacity(self.n as usize);
    queue.push(0);
    seen.insert(0);

    while let Some(i) = queue.pop() {
        for j in 0..self.n {
            if i != j && !seen.contains(&j) && (self.get(i, j) || self.get(j, i)) {
                seen.insert(j);
                queue.push(j);
            }
        }
    }

    return seen.len() == self.n as usize;
}
```

At most, we visit each node once, and since we iterate $n$ times for each node, the worst case run time is $O(n^2)$.

### Creating initial P2P

As discussed, the session layer is in charge of making the P2P network. It does this fully concurrently. Specifically, each node is able to independently decide which nodes it needs to connect to. It then sends a request to each of these nodes concurrently. This means the time it takes to send all of these requests is the same as sending just one. Furthermore, all nodes do this at the same time, so the time it takes to bring up the P2P network is the length of the longest message propagation delay.

We also retry any failed connections using exponential fall off. This means we will continue attempting to connect to the required nodes, but without overloading the network or any nodes.
