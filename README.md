# CS 6378 P2P File Sharing Project

To run the demo on UTD servers, do the following:

- Start a separate shell for `dc01.utdallas.edu`, `dc02.utdallas.edu`, and `dc03.utdallas.edu`
- Clone this repo onto one of them (they share the same file system)
- `cd` into the repo and run:
  - `./utd.sh 0` for `dc01`
  - `./utd.sh 1` for `dc02`
  - `./utd.sh 2` for `dc03`
- This will build and start the program using Cargo.

You can customize the configuration by modifying the files in `cfg`. `utd.toml` is the shared toml file used by `utd.sh`. The numbered files (`0.toml`, etc.) are loaded based on the first argument to `utd.sh`. You can add more as needed.

When you start a node for the first time, it will create a `manifest.toml` file in the `data/{NODE ID}` directory. You can use this file to add new files available for that node. To do so, place the file next to `manifest.toml` and add the file name to the `files` array in `manifest.toml`.
