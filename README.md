<a id="readme-top"></a>
<div align="center">

</div>

<!-- PROJECT LOGO -->
<br />
<div align="center">
  <img src="images/logo.svg" alt="Logo" width="300">
  <h3 align="center">Godot Iroh</h3>
  <p align="center">
    <br />
    A peer-to-peer multiplayer extension for Godot based on <a href="https://www.iroh.computer/">Iroh</a>.
  </p>
</div>

---

## Supported Platforms

Godot Iroh currently supports exports for the following platforms:

- **Windows**
- **macOS**
- **Linux**
- **Android**

### Web (WASM) Export Status

Web export support (for browsers) is currently **not supported**. More information can be found in [this issue](https://github.com/tipragot/godot-iroh/issues/48).

## Installation

### Installation via Asset Library

The addon is accessible on the Godot Asset Library [here](https://godotengine.org/asset-library/asset/3948).

### Manual Installation

- Download the latest release from [releases](https://github.com/tipragot/godot-iroh/releases).
- Extract the archive in your Godot project folder.
- That's it! You're all set!

## Usage

This plugin allows you to establish peer-to-peer multiplayer connections in Godot without relying on a centralized server, leveraging the power of [Iroh](https://www.iroh.computer/).

### Starting a Server

To start a server and set it as the multiplayer peer:

```gdscript
var server := IrohServer.start()
multiplayer.multiplayer_peer = server
```

You can retrieve the connection string (used to connect to this server) using:

```gdscript
server.connection_string()
```

After initializing the peer, you can use the [High-level multiplayer](https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html) of Godot as normal.

### Connecting as a Client

To connect to an existing server using the connection string:

```gdscript
var client = IrohClient.connect("CONNECTION_STRING")
multiplayer.multiplayer_peer = client
```

Replace `"CONNECTION_STRING"` with the string provided by the client acting as the server.

After initializing the peer, you can use the [High-level multiplayer](https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html) as normal.

### Handle Client Errors

To handle connection failures on the client side, you can connect to the connection_failed signal and get the error message with the `connection_error` function:

```gdscript
multiplayer.connection_failed.connect(func():
    print("Connection error: ", client.connection_error()))
```
This allows you to gracefully handle cases where the client cannot connect to the server.

## Examples

For more examples, see the [examples](examples/) folder in this repository.

## Contributing

Contributions are very welcome! If you find any issues, have ideas for improvements, or want to add new features, please feel free to open an issue or submit a pull request.

I would be delighted to review and merge your contributions to help make Godot Iroh even better. Whether it's bug fixes, documentation improvements, or new functionality, your help is greatly appreciated!

Thank you for helping to grow this project!
