extends VBoxContainer

signal server_started

func _ready() -> void:
	multiplayer.connection_failed.connect(_on_connection_failed)
	multiplayer.connected_to_server.connect(_on_connected_to_server)
	multiplayer.server_disconnected.connect(_on_server_disconnected)

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
