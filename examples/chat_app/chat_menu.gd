extends Control

signal server_stopped

func _ready() -> void:
	multiplayer.connected_to_server.connect(_on_connected_to_server)
	multiplayer.server_disconnected.connect(_on_server_disconnected)

func _on_server_disconnected() -> void:
	visible = false
	$MessageInterface/ClientInterface.visible = false

func _on_connected_to_server() -> void:
	visible = true
	$MessageInterface/ClientInterface.visible = true

func _on_disconnect_pressed() -> void:
	multiplayer.multiplayer_peer.close()

func _on_server_started() -> void:
	visible = true
	$MessageInterface/ServerInterface.visible = true
	var connection_string: String = multiplayer.multiplayer_peer.connection_string()
	$MessageInterface/ServerInterface/ConnectionString.text = connection_string

func _on_stop_server_pressed() -> void:
	visible = false
	multiplayer.multiplayer_peer.close()
	$MessageInterface/ServerInterface.visible = false
	for child in $MessageInterface/ScrollContainer/MessageList.get_children():
		child.queue_free()
	server_stopped.emit()

func _on_copy_clipboard_pressed() -> void:
	DisplayServer.clipboard_set(multiplayer.multiplayer_peer.connection_string())

func _on_send_message_pressed() -> void:
	var text: String = $MessageInterface/SendInterface/MessageContent.text
	if !text.is_empty(): send_message.rpc_id(1, text)
	$MessageInterface/SendInterface/MessageContent.clear()

func _on_message_content_text_submitted(new_text: String) -> void:
	if !new_text.is_empty(): send_message.rpc_id(1, new_text)
	$MessageInterface/SendInterface/MessageContent.clear()
	$MessageInterface/SendInterface/MessageContent.release_focus()
	$MessageInterface/SendInterface/MessageContent.grab_focus.call_deferred()

@rpc("any_peer", "call_local", "reliable")
func send_message(content: String) -> void:
	if multiplayer.is_server():
		var message = preload("res://message.tscn").instantiate()
		message.text = content
		$MessageInterface/ScrollContainer/MessageList.add_child(message, true)

func _on_message_list_child_entered_tree(_node: Node) -> void:
	var scroll_container: ScrollContainer = $MessageInterface/ScrollContainer
	await get_tree().process_frame
	await get_tree().process_frame
	scroll_container.set_deferred("scroll_vertical", scroll_container.get_v_scroll_bar().max_value)
