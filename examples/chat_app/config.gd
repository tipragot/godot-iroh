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

# Phase 1 & 4 Configurations
@export var lobby_topic: String = "global_lobby"
@export var allow_relay: bool = true

const CONFIG_SECTION_IDENTITY = "Identity"
const CONFIG_KEY_SECRET = "secret_key"
const CONFIG_KEY_AUTHOR = "AuthorId"

var _config := ConfigFile.new()

var nodeId: String = ""
var authorId: String = ""

# Match State
var roster: Dictionary = {}
var current_match_ticket: String = ""
var last_host_ping: float = 0.0
var watchdog: Timer

signal server_discovered(server_info: Dictionary)

func _ready() -> void:
	iroh_manager.network_started.connect(_on_network_started)
	iroh_gossip.message_received.connect(_on_gossip_received)
	iroh_docs.entry_synced.connect(_on_doc_entry_synced)
	iroh_gossip.gossip_error.connect(func(topic, err): push_error("[GOSSIP ERR] " + topic + ": " + err))
	iroh_gossip.gossip_log.connect(func(topic, msg): print("[GOSSIP LOG] " + topic + ": " + msg))
	
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
	watchdog.wait_time = 5.0
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
	# Phase 1: Join global lobby immediately upon network boot
	_mesh_and_join_lobby()

func _mesh_and_join_lobby() -> void:
	var bootstrap_peers = PackedStringArray()
	
	if OS.has_feature("editor"):
		var dir_path = "user://local_discovery"
		if not DirAccess.dir_exists_absolute(dir_path):
			DirAccess.make_dir_absolute(dir_path)
		
		var dir = DirAccess.open(dir_path)
		if dir:
			var now = Time.get_unix_time_from_system()
			for file_name in dir.get_files():
				var file_path = dir_path + "/" + file_name
				var modified_time = FileAccess.get_modified_time(file_path)
				if now - modified_time > 10.0:
					dir.remove(file_name)
		
		# 1. Drop my ID into the local pool
		var my_file = FileAccess.open(dir_path + "/" + str(OS.get_process_id()) + ".txt", FileAccess.WRITE)
		my_file.store_string(nodeId)
		my_file.close()
		
		# 2. Give the other Godot window a fraction of a second to boot and drop its file
		await get_tree().create_timer(1.5).timeout
		
		# 3. Read the pool for neighbors
		if dir.change_dir(dir_path) == OK:
			for file_name in dir.get_files():
				var file = FileAccess.open(dir_path + "/" + file_name, FileAccess.READ)
				var peer_id = file.get_as_text().strip_edges()
				if peer_id != nodeId and not peer_id.is_empty():
					bootstrap_peers.append(peer_id)
					print("[BOOTSTRAP] Found local peer: ", peer_id)

	# 4. Join the topic. Window B will use Window A's ID to instantly mesh.
	iroh_gossip.join_topic(lobby_topic, bootstrap_peers)

func _isolate_testing_environments() -> void:
	# OS.has_feature("editor") ensures this only runs during testing, 
	# not in your final exported game.
	if OS.has_feature("editor"):
		var pid = str(OS.get_process_id())
		config_path = "user://config_" + pid + ".cfg"
		cache_path = "user://iroh_cache_" + pid
		print("Isolated Debug Environment Enabled. PID: ", pid)
# ==========================================
# PHASE 3: MATCH JOINING
# ==========================================

func join_match(ticket: String, user_name: String, is_host: bool = false) -> void:
	current_match_ticket = ticket
	
	# Only guests need to tell Rust to import the document
	if not is_host:
		iroh_docs.join_document(ticket)
	
	# Tombstones use "alive": false to remove users from roster
	var payload := {
		"joined_at": Time.get_unix_time_from_system(),
		"name": user_name,
		"alive": true
	}
	iroh_docs.set_entry("roster:" + authorId, JSON.stringify(payload).to_utf8_buffer())
	roster[authorId] = payload
	
	last_host_ping = Time.get_unix_time_from_system()
	watchdog.start()

# ==========================================
# PHASE 4: ELECTION & WATCHDOG LOOP
# ==========================================

func _on_watchdog_tick() -> void:
	if roster.is_empty():
		print("[WATCHDOG] Roster empty. Waiting...")
		return
	
	var peers = roster.keys()
	peers.sort_custom(func(a, b): return roster[a].joined_at < roster[b].joined_at)
	var host_id = peers[0]
	var now = Time.get_unix_time_from_system()
	
	if host_id == authorId and allow_relay:
		print("[WATCHDOG] I am Host. Broadcasting to: ", lobby_topic)
		iroh_docs.set_entry("lobby:active_host:ping", str(now).to_utf8_buffer())
		
		var server_info := {
			"ticket": current_match_ticket,
			"players": roster.size()
		}
		iroh_gossip.broadcast(lobby_topic, JSON.stringify(server_info).to_utf8_buffer())
	else:
		print("[WATCHDOG] I am Client. Host ping age: ", now - last_host_ping, "s")
		if now - last_host_ping > 15.0:
			print("[WATCHDOG] Host dead. Dropping tombstone for: ", host_id)
			var tombstone := {"alive": false}
			iroh_docs.set_entry("roster:" + host_id, JSON.stringify(tombstone).to_utf8_buffer())

# ==========================================
# NETWORK EVENT HANDLERS
# ==========================================

func _on_doc_entry_synced(key: String, value: PackedByteArray) -> void:
	if key.begins_with("roster:"):
		var peer_id = key.trim_prefix("roster:")
		var data = JSON.parse_string(value.get_string_from_utf8())
		
		if data and data.get("alive", true):
			roster[peer_id] = data
		else:
			roster.erase(peer_id)
			
	elif key == "lobby:active_host:ping":
		last_host_ping = value.get_string_from_utf8().to_float()
	else:
		print("[DOCS SYNC] Unhandled key: ", key, " | Data: ", value.get_string_from_utf8())

func _on_gossip_received(topic: String, message: PackedByteArray) -> void:
	if topic == lobby_topic:
		var server_data = JSON.parse_string(message.get_string_from_utf8())
		if server_data and server_data.has("ticket"):
			server_discovered.emit(server_data)
