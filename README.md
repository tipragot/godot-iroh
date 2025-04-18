<a id="readme-top"></a>
<div align="center">

</div>

<!-- PROJECT LOGO -->
<br />
<div align="center">
  <img src="images/logo.svg" alt="Logo" width="300"></p>
  <h3 align="center">Godot Iroh</h3>
  <p align="center">
    <br />
    An Godot multiplayer peer extension using <a href="https://www.iroh.computer/">Iroh</a>  
  </p>
</div>

---

## Installation

Todo

## Usage

This plugin allows you to establish peer-to-peer multiplayer connections in Godot without relying on a centralized server. It uses a host to initialize the connection, leveraging the power of [Iroh](https://www.iroh.computer/).

### Starting a Host (Server)

To start a server and set it as the multiplayer peer:

```gdscript
var server := IrohServer.start()
multiplayer.multiplayer_peer = server
```

You can retrieve the connection string (to share with clients) using:

```gdscript
server.connection_string()
```

### Connecting as a Client

To connect to an existing host using the connection string:

```gdscript
var client = IrohClient.connect("CONNECTION_STRING")
multiplayer.multiplayer_peer = client
```

Replace `"CONNECTION_STRING"` with the string provided by the host server.

After initializing the peer, all other methods are the same as in the [godot documentation](https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.htm) (without the [Initializing the network](https://docs.godotengine.org/en/stable/tutorials/networking/high_level_multiplayer.html#initializing-the-network) part).

### Handle Client Errors

```
multiplayer.connection_failed.connect(func():
    print("Connection error: ", client.connection_error()))
```

## Examples

Todo