class_name Lobby
extends Control

@onready var config: IrohConfig = $Config
@onready var message_list: VBoxContainer = $ChatMenu/MessageInterface/ScrollContainer/MessageList
@onready var scroll_container: ScrollContainer = $MessageInterface/ScrollContainer

@onready var server_interface: HBoxContainer = $MessageInterface/ServerInterface
@onready var connection_string: RichTextLabel = $MessageInterface/ServerInterface/ConnectionString
@onready var connection_input: LineEdit = $PanelContainer/VBoxContainer/ConnectionMenu/JoinBox/ConnectionInput

@onready var connection_menu: VBoxContainer = $PanelContainer/VBoxContainer/ConnectionMenu
@onready var send_interface: HBoxContainer = $PanelContainer/VBoxContainer/SendInterface
@onready var message_content: LineEdit = $PanelContainer/VBoxContainer/SendInterface/MessageContent

# Called when the node enters the scene tree for the first time.
func _ready() -> void:
	config.iroh_manager.network_started.connect(_on_network_started)
	config.iroh_manager.network_start_failed.connect(_on_network_start_failed)
	start()

func start() -> void:
	config.start_or_retry()

func stop() -> void:
	pass

func _on_network_started(id: String) -> void:
	server_interface.visible = true
	connection_string.text = id # only if i need it, stored in the config.

func _on_network_start_failed(error: String) -> void:
	push_error("Iroh network failed to start: ", error)
	# TODO: Update your UI status to show the error message

func _on_disconnect_pressed() -> void:
	scroll_container.visible = true 
	# restart lobby listening
	server_interface.visible = false
	connection_menu.visible = true
	send_interface.visible = false
	connection_input.editable = true

func _on_copy_clipboard_pressed() -> void:
	#DisplayServer.clipboard_set(i probably need the invitation doc here)
	pass

func _on_send_message_pressed() -> void:
	var text: String = message_content.text
	if !text.is_empty(): _send_message(text)
	message_content.clear()

func _on_message_content_text_submitted(new_text: String) -> void:
	if !new_text.is_empty(): _send_message(new_text)
	message_content.clear()
	message_content.release_focus()
	message_content.grab_focus.call_deferred()

func _on_join_room(room: String) -> void:
	# TODO
	_on_server_selected(room)

func _on_create_room_pressed() -> void:
	var server_name = connection_input.text
	# TODO
	_on_server_selected("")

func _on_server_selected(server: String) -> void:
	scroll_container.visile = false
	connection_menu.visible = false
	connection_menu.dis
	send_interface.visible = true
	connection_string.text = "my id or server name?"

func _send_message(content: String) -> void:
	pass
	#if multiplayer.is_server():
		#var message = preload("res://message.tscn").instantiate()
		#message.text = content
		#$MessageInterface/ScrollContainer/MessageList.add_child(message, true)
