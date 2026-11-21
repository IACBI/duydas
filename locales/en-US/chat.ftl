### Conversation view and composer.

# Placeholder in the empty message box.
# Variables:
#   $channel (String) - channel name without the leading #, e.g. "general"
composer-placeholder = Message #{ $channel }
# Hint under the composer explaining keyboard shortcuts.
composer-hint = Enter to send · Shift+Enter for a new line
# Number of messages in a conversation.
# Variables:
#   $count (Number) - number of messages
message-count =
    { $count ->
        [one] { $count } message
       *[other] { $count } messages
    }
# Unread mention badge text.
# Variables:
#   $count (Number) - number of unread mentions
unread-mentions =
    { $count ->
        [one] One new mention
       *[other] { $count } new mentions
    }
