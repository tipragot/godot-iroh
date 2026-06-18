extends Control

@onready var tab_bar: TabBar = $HBoxContainer/TabBar
@onready var connection_menu: VBoxContainer = $ConnectionMenu
@onready var chat_menu: MarginContainer = $ChatMenu
@onready var lobby: Lobby = $Lobby
@onready var user_name: LineEdit = $HBoxContainer/UserName

func _ready() -> void:
	# force start lobby
	tab_bar.current_tab = 0
	user_name.text += "_"+ get_small_hash()
	lobby.config.my_name = user_name.text
	
func _on_tab_bar_tab_selected(tab: int) -> void:
	if tab == 0:
		lobby.start()
		lobby.visible = true
		chat_menu.visible = false
		connection_menu.visible = false
	else:
		lobby.stop()
		lobby.visible = false
		chat_menu.visible = false
		connection_menu.visible = true


func _on_user_name_text_changed(new_text: String) -> void:
	lobby.config.my_name = new_text
	
func get_small_hash() -> String:
	var crypto = Crypto.new()
	var bytes = crypto.generate_random_bytes(4) 
	return bytes.hex_encode()
