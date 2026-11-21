### Conversation view and composer.

# Placeholder in the empty message box.
# Variables:
#   $channel (String) - channel name without the leading #, e.g. "general"
composer-placeholder = Message #{ $channel }
# Hint under the composer explaining keyboard shortcuts.
composer-hint = Enter to send · Shift+Enter for a new line
# Number of messages in a conversation.
# Variables:
#   $count (Number) - number of messages; use only for choosing the plural form
#   $countText (String) - the same number formatted for display, e.g. "1,234"
message-count =
    { $count ->
        [one] { $countText } message
       *[other] { $countText } messages
    }
# Unread mention badge text.
# Variables:
#   $count (Number) - number of unread mentions; use only for choosing the plural form
#   $countText (String) - the same number formatted for display
unread-mentions =
    { $count ->
        [one] One new mention
       *[other] { $countText } new mentions
    }
# Label of the button that sends the composed message.
composer-send = Send
