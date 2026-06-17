class_name Lobby
extends Control

@onready var config: IrohConfig = $Config

# Chat UI
@onready var message_list: VBoxContainer = $MessageInterface/ScrollContainer/MessageList
@onready var scroll_container: ScrollContainer = $MessageInterface/ScrollContainer

# Server Browser UI
@onready var server_interface: HBoxContainer = $MessageInterface/ServerInterface
@onready var connection_string: RichTextLabel = $MessageInterface/ServerInterface/ConnectionString
@onready var connection_input: LineEdit = $PanelContainer/VBoxContainer/ConnectionMenu/JoinBox/ConnectionInput

@onready var connection_menu: VBoxContainer = $PanelContainer/VBoxContainer/ConnectionMenu
@onready var send_interface: HBoxContainer = $PanelContainer/VBoxContainer/SendInterface
@onready var message_content: LineEdit = $PanelContainer/VBoxContainer/SendInterface/MessageContent

# State tracking
var active_ticket: String = ""
var discovered_servers: Dictionary = {}

func _ready() -> void:
	# Network Boot Signals
	config.iroh_manager.network_started.connect(_on_network_started)
	config.iroh_manager.network_start_failed.connect(_on_network_start_failed)
	
	# Iroh Protocol Signals
	config.server_discovered.connect(_on_server_discovered)
	config.iroh_docs.entry_synced.connect(_on_doc_entry_synced)
	
	send_interface.visible = false
	start()

func start() -> void:
	config.start_or_retry()

func stop() -> void:
	# Future cleanup logic
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
	
	if info.ticket == active_ticket:
		print("[LOBBY] Ignoring broadcast (already inside this room).")
		return
	
	if discovered_servers.has(info.ticket):
		print("[LOBBY] Updating existing UI element for ticket.")
		var hbox = discovered_servers[info.ticket]
		hbox.get_node("Players").text = str(info.players) + " Players"
	else:
		print("[LOBBY] Spawning new UI element for ticket.")
		# Create new Server HBox
		var hbox = HBoxContainer.new()
		
		# Label 1: Server Ticket/Name
		var lbl_name = Label.new()
		lbl_name.name = "Name"
		lbl_name.text = "Room: " + info.ticket.substr(0, 8) + "..."
		lbl_name.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		
		# Label 2: Player Count
		var lbl_players = Label.new()
		lbl_players.name = "Players"
		lbl_players.text = str(info.players) + " Players"
		
		# Join Button
		var btn_join = Button.new()
		btn_join.text = "Join"
		btn_join.pressed.connect(func(): _on_join_room(info.ticket))
		
		hbox.add_child(lbl_name)
		hbox.add_child(lbl_players)
		hbox.add_child(btn_join)
		
		# Add to your UI list
		message_list.add_child(hbox)
		discovered_servers[info.ticket] = hbox

# ==========================================
# MATCH CREATION & JOINING (DOCS)
# ==========================================

func _on_create_room_pressed() -> void:
	if not active_ticket.is_empty(): 
		return 
		
	var user_name = connection_input.text
	if user_name.is_empty(): user_name = "Host"
	
	# Host Path: Create doc, grab ticket, but DO NOT call join_document
	var new_ticket = config.iroh_docs.create_document()
	active_ticket = new_ticket
	config.join_match(active_ticket, user_name, true)
	
	_transition_to_chat()

func _on_join_room(ticket: String) -> void:
	active_ticket = ticket
	var user_name = connection_input.text
	if user_name.is_empty(): user_name = "Guest"
	
	# Guest Path: Must tell Rust to import the ticket
	config.join_match(active_ticket, user_name, false)
	
	_transition_to_chat()

func _transition_to_chat() -> void:
	_clear_message_list()
	connection_menu.visible = false
	send_interface.visible = true
	scroll_container.visible = true
	connection_string.text = "Room Ticket: " + active_ticket.substr(0, 8) + "..."

# ==========================================
# CHAT SYSTEM (CRDT LWW)
# ==========================================

func _on_send_message_pressed() -> void:
	var text: String = message_content.text
	if not text.is_empty(): _send_message(text)
	message_content.clear()

func _on_message_content_text_submitted(new_text: String) -> void:
	if not new_text.is_empty(): _send_message(new_text)
	message_content.clear()
	message_content.release_focus()
	message_content.grab_focus.call_deferred()

func _send_message(content: String) -> void:
	if active_ticket.is_empty(): return
	
	# 1. Create a Time-Series CRDT Key
	var timestamp = str(Time.get_unix_time_from_system())
	var key = "chat:" + timestamp + ":" + config.authorId
	
	# 2. Package the payload
	var payload = {
		"author": config.authorId.substr(0, 5),
		"text": content
	}
	
	# 3. Write to local Rust Document (it will auto-sync to network)
	config.iroh_docs.set_entry(key, JSON.stringify(payload).to_utf8_buffer())

func _on_doc_entry_synced(key: String, value: PackedByteArray) -> void:
	# Listen for incoming CRDT chat keys
	if key.begins_with("chat:"):
		var data = JSON.parse_string(value.get_string_from_utf8())
		if data and data.has("text"):
			var msg_label = Label.new()
			msg_label.text = "[color=yellow]" + data.author + ":[/color] " + data.text
			msg_label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
			
			# If using RichTextLabel, enable bbcode
			var r_label = RichTextLabel.new()
			r_label.bbcode_enabled = true
			r_label.text = msg_label.text
			r_label.fit_content = true
			
			message_list.add_child(r_label)

# ==========================================
# UTILITIES
# ==========================================

func _on_disconnect_pressed() -> void:
	active_ticket = ""
	_clear_message_list()
	
	# Reset UI to lobby state
	scroll_container.visible = true 
	server_interface.visible = false
	connection_menu.visible = true
	send_interface.visible = false
	connection_input.editable = true
	
	# Note: In a full app, you should add a `config.leave_match()` 
	# function to stop the watchdog timer and close the Document locally.

func _on_copy_clipboard_pressed() -> void:
	if not active_ticket.is_empty():
		DisplayServer.clipboard_set(active_ticket)

func _clear_message_list() -> void:
	discovered_servers.clear()
	for child in message_list.get_children():
		child.queue_free()
