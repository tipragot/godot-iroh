class_name IrohConfig
extends Node

@onready var iroh_gossip: IrohGossip = $"../IrohGossip"
@onready var iroh_docs: IrohDocs = $"../IrohDocs"
@onready var iroh_blobs: IrohBlobs = $"../IrohBlobs"
@onready var iroh_manager: IrohManager = $"../IrohManager"

@export var config_path: String = "user://config.cfg"
@export var cache_path: String = "user://iroh_cache"
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
	
	_setup_watchdog()
	_init_config()
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

func _init_config() -> void:
	var err := _config.load_encrypted_pass(config_path, encryption_password)
	var needs_save := false
	
	if err != OK or not _config.has_section_key(CONFIG_SECTION_IDENTITY, CONFIG_KEY_SECRET):
		_config.set_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_SECRET, iroh_manager.generate_secret_key())
		needs_save = true
		
	if err == OK and _config.has_section_key(CONFIG_SECTION_IDENTITY, CONFIG_KEY_AUTHOR):
		authorId = _config.get_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_AUTHOR) as String
		iroh_docs.setup_author(authorId)
	else:
		authorId = iroh_docs.setup_author("")
		_config.set_value(CONFIG_SECTION_IDENTITY, CONFIG_KEY_AUTHOR, authorId)
		needs_save = true
		
	if needs_save:
		_config.save_encrypted_pass(config_path, encryption_password)

func _setup_watchdog() -> void:
	watchdog = Timer.new()
	watchdog.wait_time = 5.0
	watchdog.autostart = false
	watchdog.timeout.connect(_on_watchdog_tick)
	add_child(watchdog)

func _on_network_started(id: String) -> void:
	nodeId = id
	# Phase 1: Join global lobby immediately upon network boot
	iroh_gossip.join_topic(lobby_topic)

# ==========================================
# PHASE 3: MATCH JOINING
# ==========================================

func join_match(ticket: String, user_name: String) -> void:
	current_match_ticket = ticket
	iroh_docs.join_document(ticket)
	
	# Tombstones use "alive": false to remove users from roster
	var payload := {
		"joined_at": Time.get_unix_time_from_system(),
		"name": user_name,
		"alive": true
	}
	iroh_docs.set_entry("roster:" + authorId, JSON.stringify(payload).to_utf8_buffer())
	
	last_host_ping = Time.get_unix_time_from_system()
	watchdog.start()

# ==========================================
# PHASE 4: ELECTION & WATCHDOG LOOP
# ==========================================

func _on_watchdog_tick() -> void:
	if roster.is_empty(): return
	
	# Deterministic sorting to elect Host
	var peers = roster.keys()
	peers.sort_custom(func(a, b): return roster[a].joined_at < roster[b].joined_at)
	
	var host_id = peers[0]
	var now = Time.get_unix_time_from_system()
	
	if host_id == authorId and allow_relay:
		# I am Host
		iroh_docs.set_entry("lobby:active_host:ping", str(now).to_utf8_buffer())
		
		var server_info := {
			"ticket": current_match_ticket,
			"players": roster.size()
		}
		iroh_gossip.broadcast(lobby_topic, JSON.stringify(server_info).to_utf8_buffer())
	else:
		# I am Client
		if now - last_host_ping > 15.0:
			# Host dropped -> Write a tombstone. CRDT syncs it, re-triggering election.
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

func _on_gossip_received(topic: String, message: PackedByteArray) -> void:
	if topic == lobby_topic:
		var server_data = JSON.parse_string(message.get_string_from_utf8())
		if server_data and server_data.has("ticket"):
			server_discovered.emit(server_data)
