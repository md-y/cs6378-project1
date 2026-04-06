# CS 6378 P2P File Sharing Project

This document discusses the design of my P2P file sharing program. The program is written in Rust.

It is able to do the following:

- Concurrently initialize the network.
- Download files concurrently from multiple nodes, increasing download speeds.
- Handle nodes joining and leaving after the network is established, including fixing the network topology if it becomes disconnected.

## Architecture

The P2P system uses a layered architecture built on top of TCP/IP. Specifically, these are the layers:

- TCP/IP Layers: These layers consist of the standard network access, internet, and transport layers from the TCP/IP network model. Because they are standard, the program leverages built-in OS APIs. These are the lowest levels.
- Session Layer: This layer is the lowest application layer. It handles socket connections to other nodes and provides services to higher layers for broadcasting to machines, connecting to specific machines, and handling connections/disconnections. It also ensures a valid network topology and updates it as needed.
- Search Layer: This layer manages search requests and enforces hop-count rules. It also handles pathing through the network and forwarding any `adj` matrix updates.
- File Layer: This layer handles file uploads/downloads between nodes. It also handles all CLI inputs, such as `find` and `download`.

To support these layers, we have a core data structure called `Config`.
This stores information about the machine that the process is running on, the `adj` matrix, $n$, and all the routing info. When the process starts, `Config` is built by reading from the configuration files. If it is not valid, the program fails to start, including if `adj` is not a connected graph.

There is also the `FileManifest` data structure. This represents the file $F_i$ that lists which file is on which machine. This is synced with an actual file on disk, named `manifest.toml`. Each process has its own file. The data structure has many abstractions for reading and writing the data based on what is `manifest.toml`.

There is also the core `EventBus` data structure. This allows for events/messages to be sent between the layers, allowing for easy communication. Essentially, each layer has its own listener thread or coroutine that listens for events relevant to that layer (e.g. the file layer listens for file download requests). This is the main way the layers communicate to each other and are notified of global state changes. The bus supports anyone sending an event along it, and supports multiple receivers.

## Modules

Each layer has its own module.file:

- `session.rs`
  - `connections.rs` (helper structs for the session layer)
- `search.rs`
- `file.rs`

Beyond these modules, we have these utility modules that support the layers:

- `config.rs` (the core module for `Config` that manages deserialization and verification)
- `adj.rs` (`adj`-specific logic used by the `Config` module, mostly in charge of verifying `adj` is connected)
- `message.rs` (contains `Message` enum that specifies each message type)
- `bus.rs` (contains the `EventBus` struct and all event types)
- `file_manifest.rs` (contains the `FileManifest` struct and all associated logic)
- `logger.rs` (the basic logging template used by all the layers)

We will go into more detail on these later.

Finally, `main.rs` is the entry point of the application. It does not do much other than creating the `Config` object and starting all layers.

### `config.rs` and `adj.rs`

All machine information is configured in `.toml` files. This makes it easy to configure the program for different environments. In addition, you can merge configs together. This means we can have `adj` and routing information in shared `.toml` files. For example, on the UTD servers, there is a shared config named `utd.toml` that contains `adj` and the node ID to socket address mappings. Each node then has its own dedicated `0.toml`, `1.toml`, etc. files that configure settings for that particular node, mainly just the node ID.

When we start the program, each argument is interpreted as a config file path. We then use the `toml` and `serde` crates to deserialize these files into unverified config and `adj` structs. These can be merged together into a final candidate config.

After merging, we verify if the config is correct. This includes (but is not limited to):

- Checking all required settings are present (e.g. node ID, `adj` section, `routes` section, etc.)
- Checking if `adj` is connected and is $n$ by $n$
- Checking if the node ID is less than $n$ (since node ID is 0-indexed). If it is, we consider this node an "outside" node, and such nodes require the `target_node` setting which dictates which node it talks to in order to join the network.

`adj` has its own section that has two settings:

- $n$ (the total number of nodes)
- matrix (a 2D matrix representing `adj`. This is converted into a 1D vector after deserialization to save memory.)

`routes` is another section that specifies the domain and port of each machine. Each value is a map between the node ID and the authority string (domain + port). For example:

```toml
0 = "dc01.utdallas.edu:50890"
1 = "dc02.utdallas.edu:50890"
2 = "dc03.utdallas.edu:50890"
```

`main.rs` triggers the config deserialization and saves the verified struct as a shared singleton available to all layers.
This also means all updates to `Config` and `Adj` affect all layers.

### `session.rs`

When the session layer is initialized, is scans `adj` to determine which nodes it should send connection requests to. Since we only need one node to initiate a request in order to establish a bidirectional TCP channel, we need to decide on a total ordering. This is done using these rules:

- If $\operatorname{Adj}[i, j] = 1$ but $\operatorname{Adj}[j, i] \neq 1$, then node $i$ requests to connect to node $j$.
- If $\operatorname{Adj}[i, j] = 1$ and $\operatorname{Adj}[j, i] = 1$, then node $i$ requests to connect to node $j$ if and only if $i < j$.

To connect to a node, we send an `InitRequest` message. All messages are instances of the `Message` struct, and each has a body from the `MessageBody` enum. The messages are serialized using `bson`, and `serde` automatically differentiates each message body type using its `tag` trait.

I chose to use `bson` because I am most familiar with using JSON for network requests. However, we are sending files over the network for this project, and JSON does not support binary data, so it would add significant overhead. Therefore, `bson` is a good alternative since it supports binary data and automatically includes length information when serialized, allowing for easy detection of when messages end.

In addition to sending connection requests, the session layer also listens for any new TCP connections in a separate coroutine. It accepts any request and then listens for a `InitRequest` message. If the sender correctly sends a `InitRequest` message, the recipient node returns a `InitResponse` message. This is the handshake for all nodes in the application. If the sender fails to do this handshake, its request is ignored.

If a connecting node is considered an "outside" node (either its node ID as outside the bounds of `adj` or if it previously dropped out of the network), we update `adj` to incorporate this node. This means expanding the size of `adj` if needed. This new `adj` value is broadcasted on all channels, and since the search layer forwards these requests, all nodes will receive this new `adj`.

Once all the required initial connections have been established (as determined by the initial `adj` configuration), the node is considered ready, and the higher layers are enabled. This is done by broadcasting the `NetworkEstablished` event.

All successful connections are recorded in a shared hash map. This hash map is protected behind a mutex since connections are in separate coroutines. This hash map and all other connection-related logic is managed by the `ConnectionManager` struct. Each connection has a `Connection` struct that handles message serialization/deserialization and sending/reading. Each `Connection` struct has a worker coroutine that continuously listens to messages and emits an event when one is received. However, since reading is a blocking action, it also listens for any internal interrupt events, usually done to free up the stream for writing a message.

### `search.rs`

The search layer handles navigating the network topology. It accepts three types of messages from the event bus:

- `SearchRequest` (these messages include the sender and desired files, and each node that receives it along the way will forward it)
- `SearchResponse` (the response to `SearchRequest` containing file information and the node that possesses it)
- `AdjUpdate` (these messages include the updated `adj` matrix that gets generated from nodes joining/leaving)

All three types of messages are forwarded as needed. `SearchRequest` and `AdjUpdate` are broadcasted on all channels, and `SearchResponse` is forwarded back to the sender of the corresponding `SearchRequest`.
However, if the node has already seen a specific message, it does nothing with it. This is tracked using a message `key` which is generated by the original sender based on its local clock. The key also includes the original sender's ID, so it is globally unique. Seen messages' keys are stored in a `HashSet`. We also ignore messages whose hop count reached 0.

We also "consume" a `SearchRequest` if we have the file contain in the request, and we consume a `SearchResponse` if we were the original requestor. Consuming just involves sending the appropriate response message and/or sending the appropriate event to the next layer along the bus.

The layer sends out `SearchRequest` messages as a broadcast along its edges if it does not know where the file is located. This means that it can receive many `SearchResponse` messages per `SearchRequest` if multiple nodes have the file. To account for this (and to accept random `SearchResponse` messages), we store the received file information in another mutex-protected hash map. We use this stored information for the file layer.

### `file.rs`

The file layer also handles two message types:

- `FileRequest` (message contains which file the sender wants and which slice of it)
- `FileResponse` (slice of the requested file)

At the beginning, the node does not know where a file is. The user can initiate a search using `find <filename>`.
This causes the file layer to repeatedly send search requests to the search layer using exponentially increasing hop counts.
If we find the file, we store this information in a `HashMap` to be used by the `download` command. Optionally, we can set the hop count manually using `find <filename> [hop count index]`.

When the user runs `download <node id 1> [node id 2] ...`, the file layer looks at the stored file information from the `find` command. The user specifies which nodes to download from, and the file layer concurrently sends `FileRequest` to each of these nodes. If a connection is not established with the target node, it uses the session layer to establish it first before requesting the file. Afterwards, the layer kills these temporary connections.

Each `FileRequest` specifies which slice of the file to download. Each slice is equally sized, so if there are two nodes being downloaded from, the first node returns the first 50% of the file, and the other returns the other 50%.

Once the file is done downloading, the file layer notifies the user, writes the file to disk, and updates `FileManifest`. This means the node can then serve the file to other nodes if needed.

The file layer also handles other CLI commands. They are not as related to file downloading, but the logic for them all is very similar.

- `exit` (gracefully shuts down this node by removing itself from `adj`, repairing it as necessary, and broadcasting the new matrix using the session layer)
- `adj` (prints the current `adj` matrix)
- `repair` (attempts to reconnect to any nodes that have unexpectedly disconnected)
- `help` (prints all command info)

### `bus.rs` (and other buses)

The event bus is based around Tokio's [broadcast channel](https://docs.rs/tokio/latest/tokio/sync/broadcast/fn.channel.html). However, there are a few abstractions on top of the raw channel data structure that make it easier to use. Mainly, the `wait_for` function takes a closure that filters for the exact event type required by the subscriber. This makes it easy for each layer to only listen for the events it cares about.

Outside the main event bus, we have local buses per connection. These are used to force the TCP connection worker thread to release the mutex on the TCP stream so we can either write to the stream, or kill the connection.

Overall, I chose to use this event bus-based system instead of callbacks. In my experience, callbacks are an antipattern and are harder to scale.

These are all the event types for the main event bus:

- `NewConnection` (emitted by `ConnectionManager` whenever a connection finishes the join handshake, and is consumed by the session layer to update `adj` as needed)
- `ConnectionClosed` (emitted by `ConnectionManager` whenever a connection closes, and is also consumed by the session layer to again update `adj` as needed )
- `MessageReceived` (emitted by `ConnectionManager` whenever a message is received, and consumed by all layers. The event contains the message type and content.),
- `NetworkEstablished` (emitted by the session layer whenever the required initial connections are established, and consumed by the other layers to start their initialization),
- `FileFound` (emitted by the search layer when a file is found, and consumed by the file layer to save this information),
- `Shutdown` (emitted by the file layer when the user requests to exit, and consumed by all indefinite threads/coroutines so they stop running),
- `SocketClosed` (internally used by `ConnectionManager` to detect when the raw TCP stream of a connection is closed),

## Algorithms

This section discusses specific algorithms used in the program.
These are mainly ones that were specifically instructed to be mentioned.

### Adj Connectivity Check

Upon initialization, each node checks if `adj` is valid by ensuring it has the correct dimensions ($n$ by $n$) and if it is connected (i.e. it is a single connected component).

For this latter check, we do a simple breadth-first search. We keep track of each node we visit and once the queue of unvisited nodes is empty, we see if we visited $n$ nodes. We add nodes to the queue if they have not been visited already, and if either $\operatorname{Adj}[i, j]=1$ or $\operatorname{Adj}[j, i]=1$ since TCP connections are bidirectional, so the edges are undirected. We ignore the diagonal ($i=j$) and all deactivated nodes (ones that have dropped out of the network).

Below is the code that does this check.

```rust
// self is the Adj struct
pub fn is_connected(&self) -> bool {
    let mut seen = HashSet::<u32>::new();
    let mut queue = Vec::<u32>::with_capacity(self.n as usize);
    for i in 0..self.n {
        if self.is_activated(&i) {
            queue.push(i);
            seen.insert(i);
            break;
        }
    }

    while let Some(i) = queue.pop() {
        for j in 0..self.n {
            if i != j && !seen.contains(&j) && (self.get(i, j) || self.get(j, i)) {
                seen.insert(j);
                queue.push(j);
            }
        }
    }

    let required_count = self.n as usize - self.deactivated.len();
    return seen.len() == required_count;
}
```

At most, we visit each node once, and since we iterate $n$ times for each node, the worst case run time is $O(n^2)$.

### Creating Initial P2P

As discussed, the session layer is in charge of making the P2P network. It does this fully concurrently. Specifically, each node is able to independently decide which nodes it needs to connect to. It then sends a request to each of these nodes concurrently. This means the time it takes to send all of these requests is the same as sending just one. Furthermore, all nodes do this at the same time, so the time it takes to bring up the P2P network is the length of the longest message propagation delay.

We also retry any failed connections using exponential fall off. This means we will continue attempting to connect to the required nodes, but without overloading the network or any nodes.

### Hop Count Delay

The hop count delay for each file search request is simply:

$$
t_{\text{hop\_count}}=2\text{second} * (\log_2(\text{hop\_count}) + 1)
$$

### Fixing Disconnected Graph

To detect if `adj` is disconnected, we re-use `is_connected`. If it is disconnected, we repair it by joining all disconnected components.

First, we need to find the components. This is done by keeping track of the nodes in each component in a `HashSet`.
Initially, every active node is in a set by itself. We ignore deactivated nodes since they are not considered an active part of the network. All sets are stored in a vector.

We then iterate over each cell in `adj`. If there is an edge between node $i$ and node $j$, and the nodes are in two different sets, we replace the two sets with one unioned set.

At the end, if there is more than one component, the graph is disconnected.

This is the implemented function for getting the components:

```rust
pub fn get_components(&self) -> Vec<HashSet<u32>> {
    let mut components: Vec<HashSet<u32>> = Vec::new();
    for i in 0..self.n {
        if self.is_activated(&i) {
            components.push(HashSet::from([i]));
        }
    }

    for i in 0..self.n {
        for j in 0..self.n {
            if i == j || (!self.get(i, j) && !self.get(j, i)) {
                continue;
            }

            let i_c_idx = components.iter().position(|s| s.contains(&i)).unwrap();
            let j_c_idx = components.iter().position(|s| s.contains(&j)).unwrap();
            if i_c_idx == j_c_idx  {
                continue;
            }

            let lesser = min(i_c_idx, j_c_idx);
            let greater = max(i_c_idx, j_c_idx);
            let removed_set = components.swap_remove(greater);
            components[lesser] = &components[lesser] | &removed_set;
        }
    }

    return components;
}
```

This runs in $O(n^2)$ time. The speed could be improved, mainly by using disjointed set unioning, but this works fine for our purposes.

Next, we can repair the graph. The repair is done using a hub-and-spoke technique. This means we find the largest component and connect all other components to it. This minimizes the diameter of the resulting graph.

More specifically, we find the largest component and then find the node with the greatest degree in it. This is our "core" node. For each other component, we also find the largest node in that component and connect it to the core node.

This is the implemented repair function:

```rust
pub fn repair(&mut self) {
    let components = self.get_components();
    if components.len() == 1 {
        return;
    }

    let hub = components.iter().max_by_key(|set| set.len()).unwrap();

    let core = hub.iter().max_by_key(|id| self.node_degree(**id)).unwrap();

    for c in &components {
        if c == hub {
            continue;
        }

        let node = c.iter().max_by_key(|id| self.node_degree(**id)).unwrap();
        self.set(*core, *node, true);
    }
}
```

This part of the algorithm takes $O(k)$ time where $k$ is the number of components. Since $k \leq n$, this means it takes $O(n)$ time.
However, it takes $O(n^2)$ time to find the components, so the total performance is $O(n^2)$.
