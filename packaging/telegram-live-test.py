"""Explicitly requested Telegram discovery/test; credentials enter over stdin."""
import json
import getpass
import os
import re
import sys
import time
import urllib.error
import urllib.request


def api(token, method, payload):
    request = urllib.request.Request(
        'https://api.telegram.org/bot' + token + '/' + method,
        data=json.dumps(payload).encode('utf-8'),
        headers={'Content-Type': 'application/json; charset=utf-8'},
    )
    try:
        with urllib.request.urlopen(request, timeout=25) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        # Never emit exception strings: they can contain the credential URL.
        print(json.dumps({'ok': False, 'http_status': error.code}))
        sys.exit(2)
    except Exception:
        print(json.dumps({'ok': False, 'error': 'network_unavailable'}))
        sys.exit(3)
    if result.get('ok') is not True:
        print(json.dumps({'ok': False, 'error_code': result.get('error_code')}))
        sys.exit(2)
    return result['result']


def main():
    token = getpass.getpass('Token (hidden): ').strip() if sys.stdin.isatty() else sys.stdin.readline().strip()
    if not re.fullmatch(r'\d+:[A-Za-z0-9_-]+', token):
        print('{"ok":false,"error":"token_format"}')
        return 2
    if sys.argv[1] == 'discover':
        bot = api(token, 'getMe', {})
        webhook = api(token, 'getWebhookInfo', {})
        updates = api(token, 'getUpdates', {'limit': 100, 'timeout': 0, 'allowed_updates': ['message']})
        chats = {}
        for update in updates:
            message = update.get('message', {})
            text = message.get('text', '')
            if text.split(' ', 1)[0].split('@')[0] != '/start':
                continue
            if message.get('date', 0) < time.time() - 1800:
                continue
            chat = message.get('chat', {})
            if chat.get('type') != 'private':
                continue
            chats[chat['id']] = {
                'chat_id': chat['id'], 'name': chat.get('first_name', ''),
                'username': chat.get('username', ''), 'date': message.get('date'),
            }
        print(json.dumps({'ok': True, 'bot_username': bot.get('username'), 'webhook_configured': bool(webhook.get('url')), 'updates_returned': len(updates), 'recent_start_chats': list(chats.values())}, ensure_ascii=True))
    elif sys.argv[1] == 'send':
        chat_id = int(sys.argv[2])
        result = api(token, 'sendMessage', {
            'chat_id': chat_id,
            'text': 'Utility Rebuild | TEST\nTelegram notification test successful.\nThis is a test message, not a Windows installation completion report.',
            'disable_web_page_preview': True,
        })
        print(json.dumps({'ok': True, 'chat_id': result['chat']['id'], 'message_id': result['message_id']}))
    return 0


if __name__ == '__main__':
    sys.exit(main())
