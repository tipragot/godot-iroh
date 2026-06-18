class_name IrohConfig
extends Node

@onready var iroh_gossip: IrohGossip = $"../IrohGossip"
@onready var iroh_docs: IrohDocs = $"../IrohDocs"
@onready var iroh_blobs: IrohBlobs = $"../IrohBlobs"
@onready var iroh_manager: IrohManager = $"../IrohManager"

@export var config_path: String = "config.cfg"
@export var cache_path: String = "iroh_cache"
@export var encryption_password: String = "default_demo_password_123"
@export var auto_start: bool = false

@export var global_topic: String = "global_lobby"
@export var my_name: String = "Player"

const CONFIG_SECTION_IDENTITY = "Identity"
const CONFIG_KEY_SECRET = "secret_key"
const CONFIG_KEY_AUTHOR = "AuthorId"

var _config := ConfigFile.new()

var nodeId: String = ""
var authorId: String = ""

# Lobby State
var is_host: bool = false
var active_room_topic: String = ""
var watchdog: Timer

signal server_discovered(info: Dictionary)
signal chat_received(author: String, text: String)
signal game_started(ticket: String)

func _ready() -> void:
	iroh_manager.network_started.connect(_on_network_started)
	iroh_gossip.message_received.connect(_on_gossip_received)
	iroh_docs.entry_synced.connect(_on_doc_entry_synced)
	
	_setup_watchdog()
	_isolate_testing_environments()
	_ensure_secret_key()
	
	if auto_start: start_or_retry()

func start_or_retry() -> void:
	if nodeId.is_empty(): 
		iroh_manager.start_network(
			iroh_docs.get_path(),
			iroh_gossip.get_path(),
			iroh_blobs.get_path(),
			cache_path,
			_get_secret_key()
		)

func _get_secret_key() -> PackedByteArray:
	_config.load_encrypted_pass(config_path, encryption_password)
	return _config.get_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_SECRET) as PackedByteArray

func _ensure_secret_key() -> void:
	var err := _config.load_encrypted_pass(config_path, encryption_password)
	if err != OK or not _config.has_section_key(CONFIG_SECTION_IDENTITY, CONFIG_KEY_SECRET):
		_config.set_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_SECRET, iroh_manager.generate_secret_key())
		_config.save_encrypted_pass(config_path, encryption_password)

func _setup_watchdog() -> void:
	watchdog = Timer.new()
	watchdog.wait_time = 3.0
	watchdog.autostart = false
	watchdog.timeout.connect(_on_watchdog_tick)
	add_child(watchdog)

func _on_network_started(id: String) -> void:
	nodeId = id
	var saved_author = _config.get_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_AUTHOR, "") as String
	authorId = iroh_docs.setup_author(saved_author)
	
	if authorId != saved_author:
		_config.set_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_AUTHOR, authorId)
		_config.save_encrypted_pass(config_path, encryption_password)
	
	_mesh_and_join_global()

func _mesh_and_join_global(manual_bootstrap: String = "") -> void:
	# TODO manual_bootstrap: load cached friends?
	var bootstrap_peers = PackedStringArray()
	if not manual_bootstrap.is_empty():
		bootstrap_peers.append(manual_bootstrap)
		
	if OS.has_feature("editor"):
		var dir_path = "local_discovery"
		DirAccess.make_dir_absolute(dir_path)
		
		var dir = DirAccess.open(dir_path)
		if dir:
			var now = Time.get_unix_time_from_system()
			for file_name in dir.get_files():
				var file_path = dir_path + "/" + file_name
				if now - FileAccess.get_modified_time(file_path) > 10.0:
					dir.remove(file_name)
			
			var my_file = FileAccess.open(dir_path + "/" + str(OS.get_process_id()) + ".txt", FileAccess.WRITE)
			my_file.store_string(nodeId)
			my_file.close()
			
			await get_tree().create_timer(1.5).timeout
			
			for file_name in dir.get_files():
				var peer_id = FileAccess.open(dir_path + "/" + file_name, FileAccess.READ).get_as_text().strip_edges()
				if peer_id != nodeId and not peer_id.is_empty():
					bootstrap_peers.append(peer_id)

	iroh_gossip.join_topic(global_topic, bootstrap_peers)

func _isolate_testing_environments() -> void:
	if OS.has_feature("editor"):
		var pid = str(OS.get_process_id())
		config_path = "config_" + pid + ".cfg"
		cache_path = "iroh_cache_" + pid

# ==========================================
# LOBBY & GOSSIP ROUTING
# ==========================================

func host_room(roomId: String) -> void:
	is_host = true
	active_room_topic = "room_" + roomId
	iroh_gossip.join_topic(active_room_topic, PackedStringArray())
	watchdog.start()

func join_room(target_topic: String) -> void:
	is_host = false
	active_room_topic = target_topic
	
	var host_node_id = target_topic.trim_prefix("room_")
	var peers = PackedStringArray([host_node_id])
	
	iroh_gossip.join_topic(active_room_topic, peers)

func leave_room(target_topic: String) -> void:
	iroh_gossip.leave_topic(target_topic)
	if target_topic == active_room_topic:
		active_room_topic = ""
	var payload = {
		"type": "disconnect",
		"author": my_name
	}
	iroh_gossip.broadcast(target_topic, JSON.stringify(payload).to_utf8_buffer())
	
func send_chat(text: String) -> void:
	if active_room_topic.is_empty(): return
	
	var payload = {
		"type": "chat",
		"author": my_name,
		"text": text
	}
	iroh_gossip.broadcast(active_room_topic, JSON.stringify(payload).to_utf8_buffer())

func _on_watchdog_tick() -> void:
	if is_host:
		var ad = {
			"type": "server_ad",
			"topic": active_room_topic,
			"host": my_name
		}
		iroh_gossip.broadcast(global_topic, JSON.stringify(ad).to_utf8_buffer())

func _on_gossip_received(topic: String, message: PackedByteArray) -> void:
	var msg = JSON.parse_string(message.get_string_from_utf8())
	if not msg: return
	
	if topic == global_topic and msg.get("type") == "server_ad":
		if msg.topic != active_room_topic:
			server_discovered.emit(msg)
			
	elif topic == active_room_topic:
		match msg.get("type"):
			"chat":
				chat_received.emit(msg.author, msg.text)
			"game_start":
				_start_game_client(msg.ticket)

# ==========================================
# DOCS & GAME START
# ==========================================

func start_game_host() -> void:
	if not is_host: return
	
	var ticket = iroh_docs.create_document()
	var payload = {
		"type": "game_start",
		"ticket": ticket
	}
	iroh_gossip.broadcast(active_room_topic, JSON.stringify(payload).to_utf8_buffer())
	_start_game_client(ticket)

func _start_game_client(ticket: String) -> void:
	iroh_docs.join_document(ticket)
	game_started.emit(ticket)

func _on_doc_entry_synced(key: String, value: PackedByteArray) -> void:
	# Future game state syncing drops in here exclusively. 
	pass
