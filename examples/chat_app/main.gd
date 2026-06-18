extends Control

@onready var tab_bar: TabBar = $TabBar
@onready var connection_menu: VBoxContainer = $ConnectionMenu
@onready var chat_menu: MarginContainer = $ChatMenu
@onready var lobby: Lobby = $Lobby

func _ready() -> void:
	# force start lobby
	tab_bar.current_tab = 0

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
