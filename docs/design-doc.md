# Architecture

The P2P system uses a layered architecture built on top of TCP/IP. Specifically, these are the layers:

- TCP/IP Layers: These layers consist of the standard network access, internet, and transport layers from the TCP/IP network model. Because they are standard, the program leverages built-in OS APIs. These are the lowest levels.
- Session Layer: This layer is the lowest application layer. It handles socket connections to other nodes and provides services to higher layers for broadcasting to machines, connecting to specific machines, and handling connections/disconnections. It also ensures a valid network topology.
- Search Layer: This layer manages search requests and enforces hop-count rules. It also handles pathing through the network.
- File Layer: This layer handles file uploads/downloads between nodes.

# Algorithms

This section discusses specific algorithms used in the program.
These are mainly ones that were specifically instructed to be mentioned.

## Adj Connectivity Check

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
