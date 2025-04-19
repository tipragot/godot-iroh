extends HBoxContainer

var text: String:
	get: return $MessageContent/Margin/Content.text
	set(value): $MessageContent/Margin/Content.text = value

func _ready() -> void:
	if multiplayer.is_server():
		$MessageDelete.visible = true

func _on_message_delete_pressed() -> void:
	queue_free()
