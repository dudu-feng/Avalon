from loop import react_loop, session_manage

if __name__ == "__main__":
    print("Avalon Agent 已启动！输入 exit 退出。\n")

    while True:
        user_input = input("你: ")
        if user_input.lower() in ["exit", "quit", "退出"]:
            # 退出前归档当前会话
            session_manage.save_current_session()
            print("再见！")
            break

        if not user_input.strip():
            continue

        try:
            chat_history = react_loop.react_loop(user_input)

            # 实时保存当前会话
            session_manage.update_current_session(chat_history)

        except Exception as e:
            print(f"错误: {e}\n")
