from loop import react_loop, session_manage

if __name__ == "__main__":
    # 初始化当前会话
    session_manage.init_session()

    print("=" * 50)
    print("  Avalon — 个人 AI 助手")
    print("  输入消息开始对话，/help 查看命令")
    print("=" * 50)
    print()

    while True:
        try:
            user_input = input("You > ").strip()
        except (EOFError, KeyboardInterrupt):
            # 用户退出（Ctrl+C 或 Ctrl+D）
            print()
            break

        if not user_input:
            continue

        # 处理命令
        if user_input == "/compress":
            session_manage.session_compress()
            continue
        if user_input in ("/exit", "/quit"):
            break
        if user_input == "/help":
            print("命令列表：")
            print("  /exit, /quit   退出")
            print("  /help          显示帮助")
            print("  /compress       压缩当前会话")
            print()
            continue
        chat_history = react_loop.react_loop(user_input)
        session_manage.update_current_session(chat_history)
        # 自动压缩检查：输入 token 超过阈值时自动触发压缩
        session_manage.auto_compress_check_from_history(chat_history)
    # 退出前归档当前会话
    print("正在保存本次对话")
    session_manage.save_current_session()
    print("再见。")