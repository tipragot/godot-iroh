extends VBoxContainer

signal server_started

var manager: IrohManager
var gossip : IrohGossip
var blobs: IrohBlobs

func _ready() -> void:
	multiplayer.connection_failed.connect(_on_connection_failed)
	multiplayer.connected_to_server.connect(_on_connected_to_server)
	multiplayer.server_disconnected.connect(_on_server_disconnected)
	manager = IrohManager.new()
	add_child(manager)
	var doc = IrohDocs.new()
	add_child(doc)
	blobs = IrohBlobs.new()
	add_child(blobs)
	gossip = IrohGossip.new()
	add_child(gossip)
	manager.start_network(doc.get_path(), gossip.get_path(), blobs.get_path())

func _on_connection_failed() -> void:
	$ErrorLabel.text = multiplayer.multiplayer_peer.connection_error()
	$JoinBox/ConnectionString.editable = true
	$JoinBox/JoinRoom.disabled = false
	$CreateRoom.disabled = false

func _on_server_disconnected() -> void:
	visible = true
	$JoinBox/ConnectionString.editable = true
	$JoinBox/JoinRoom.disabled = false
	$CreateRoom.disabled = false

func _on_connected_to_server() -> void:
	visible = false

func _on_join_room_pressed() -> void:
	var client := IrohClient.connect($JoinBox/ConnectionString.text)
	multiplayer.multiplayer_peer = client
	$JoinBox/ConnectionString.editable = false
	$JoinBox/JoinRoom.disabled = true
	$CreateRoom.disabled = true

func _on_create_room_pressed() -> void:
	var server := IrohServer.start()
	multiplayer.multiplayer_peer = server
	visible = false
	server_started.emit()

func _on_server_stopped() -> void:
	visible = true
	
func _on_user_joined_swarm():
	# Use Iroh's Gossip to broadcast to the P2P swarm
	gossip.join_topic("my-garden-swarm")
	gossip.broadcast("Hello World!".to_utf8_buffer())

func _on_database_changed():
	# Put a file in Iroh Blobs and get back a hash ticket
	var ticket = blobs.host_file("/user/data/kuzu.db")
	print("Share this ticket to sync db: ", ticket)
