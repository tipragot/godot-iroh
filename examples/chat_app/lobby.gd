class_name Lobby
extends Control

@onready var config: IrohConfig = $Config

# Chat UI
@onready var message_list: VBoxContainer = $MessageInterface/ScrollContainer/MessageList
@onready var scroll_container: ScrollContainer = $MessageInterface/ScrollContainer
@onready var client_interface: HBoxContainer = $MessageInterface/ClientInterface

# Server Browser UI
@onready var server_interface: HBoxContainer = $MessageInterface/ServerInterface
@onready var connection_string: RichTextLabel = $MessageInterface/ServerInterface/ConnectionString
@onready var connection_input: LineEdit = $PanelContainer/VBoxContainer/ConnectionMenu/JoinBox/ConnectionInput

@onready var connection_menu: VBoxContainer = $PanelContainer/VBoxContainer/ConnectionMenu
@onready var send_interface: HBoxContainer = $PanelContainer/VBoxContainer/SendInterface
@onready var message_content: LineEdit = $PanelContainer/VBoxContainer/SendInterface/MessageContent

# State tracking
var active_room_topic: String = ""
var discovered_servers: Dictionary = {}

func _ready() -> void:
	# Network Boot Signals
	config.iroh_manager.network_started.connect(_on_network_started)
	config.iroh_manager.network_start_failed.connect(_on_network_start_failed)
	
	# Iroh Protocol Signals
	config.server_discovered.connect(_on_server_discovered)
	config.chat_received.connect(_on_chat_received)
	config.game_started.connect(_on_game_started)
	
	send_interface.visible = false
	start()

func start() -> void:
	config.start_or_retry()

func stop() -> void:
	pass

# ==========================================
# NETWORK & UI INITIALIZATION
# ==========================================

func _on_network_started(id: String) -> void:
	server_interface.visible = true
	connection_string.text = "My Node: " + id.substr(0, 8) + "..."

func _on_network_start_failed(error: String) -> void:
	push_error("Iroh network failed to start: ", error)

# ==========================================
# SERVER BROWSER (GOSSIP)
# ==========================================

func _on_server_discovered(info: Dictionary) -> void:
	print("[LOBBY] Gossip received server info: ", info)
	
	if info.topic == active_room_topic:
		print("[LOBBY] Ignoring broadcast (already inside this room).")
		return
	
	if discovered_servers.has(info.topic):
		print("[LOBBY] Updating existing UI element for topic.")
		var hbox = discovered_servers[info.topic]
		hbox.get_node("HostName").text = "Host: " + info.host
	else:
		print("[LOBBY] Spawning new UI element for topic.")
		var hbox = HBoxContainer.new()
		
		var lbl_name = Label.new()
		lbl_name.name = "Name"
		lbl_name.text = "Room: " + info.topic.substr(0, 12) + "..."
		lbl_name.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		
		var lbl_host = Label.new()
		lbl_host.name = "HostName"
		lbl_host.text = "Host: " + info.host
		
		var btn_join = Button.new()
		btn_join.text = "Join"
		btn_join.pressed.connect(func(): _on_join_room(info.topic))
		
		hbox.add_child(lbl_name)
		hbox.add_child(lbl_host)
		hbox.add_child(btn_join)
		
		message_list.add_child(hbox)
		discovered_servers[info.topic] = hbox

# ==========================================
# MATCH CREATION & JOINING (GOSSIP LOBBY)
# ==========================================

func _on_create_room_pressed() -> void:
	if not active_room_topic.is_empty(): 
		return 
		
	var user_name = connection_input.text
	if user_name.is_empty(): user_name = "Host"
	
	config.host_room(user_name)
	active_room_topic = config.active_room_topic
	
	_transition_to_chat()

func _on_join_room(topic: String) -> void:
	active_room_topic = topic
	var user_name = connection_input.text
	if user_name.is_empty(): user_name = "Guest"
	
	config.join_room(active_room_topic, user_name)
	client_interface.visible = true
	_transition_to_chat()

func _transition_to_chat() -> void:
	_clear_message_list()
	connection_menu.visible = false
	send_interface.visible = true
	scroll_container.visible = true
	connection_string.text = "Lobby Topic: " + active_room_topic.substr(0, 12) + "..."

# ==========================================
# CHAT SYSTEM (GOSSIP)
# ==========================================

func _on_send_message_pressed() -> void:
	var text: String = message_content.text
	if not text.is_empty(): 
		config.send_chat(text)
		_on_chat_received(config.my_name, text) # Echo locally
	message_content.clear()

func _on_message_content_text_submitted(new_text: String) -> void:
	if not new_text.is_empty(): 
		config.send_chat(new_text)
		_on_chat_received(config.my_name, new_text) # Echo locally
	message_content.clear()
	message_content.release_focus()
	message_content.grab_focus.call_deferred()

func _on_chat_received(author: String, text: String) -> void:
	print("[LOBBY] Chat received from ", author, ": ", text)
	
	var r_label = RichTextLabel.new()
	r_label.bbcode_enabled = true
	r_label.text = "[color=yellow]" + author + ":[/color] " + text
	r_label.fit_content = true
	r_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	
	message_list.add_child(r_label)

# ==========================================
# GAME START (DOCS)
# ==========================================

func _on_start_game_pressed() -> void:
	if config.is_host:
		print("[LOBBY] Host starting game, creating Document...")
		config.start_game_host()

func _on_game_started(ticket: String) -> void:
	print("[LOBBY] Game started! Doc Ticket: ", ticket)
	connection_string.text = "Game Running: " + ticket.substr(0, 8) + "..."

# ==========================================
# UTILITIES
# ==========================================

func _on_disconnect_pressed() -> void:
	_clear_message_list()
	config.leave_room(active_room_topic)
	active_room_topic = ""
	
	# Reset UI to lobby state
	client_interface.visible = false
	scroll_container.visible = true 
	server_interface.visible = false
	connection_menu.visible = true
	send_interface.visible = false
	connection_input.editable = true

func _on_copy_clipboard_pressed() -> void:
	if not active_room_topic.is_empty():
		DisplayServer.clipboard_set(active_room_topic)

func _clear_message_list() -> void:
	discovered_servers.clear()
	for child in message_list.get_children():
		child.queue_free()
