# Architecture

The P2P system uses a layered architecture built on top of TCP/IP. Specifically, these are the layers:

- TCP/IP Layers: These layers consist of the standard network access, internet, and transport layers from the TCP/IP network model. Because they are standard, the program leverages built-in OS APIs. These are the lowest levels.
- Session Layer: This layer is the lowest application layer. It handles socket connections to other nodes and provides services to higher layers for broadcasting to machines, connecting to specific machines, and handling connections/disconnections. It also ensures a valid network topology.
- Search Layer: This layer manages search requests and enforces hop-count rules. It also handles pathing through the network.
- File Layer: This layer handles file uploads/downloads between nodes.
