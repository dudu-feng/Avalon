from loop import react_loop

if __name__ == "__main__":
    print("Avalon Agent 已启动！输入 exit 退出。\n")

    while True:
        user_input = input("你: ")
        if user_input.lower() in ["exit", "quit", "退出"] and user_input.strip():
            print("再见！")
            break

        if not user_input.strip():
            continue

        try:
            chat_history = react_loop.react_loop(user_input)
            print(f"chat_history{chat_history}")

            
        except Exception as e:
            print(f"错误: {e}\n")
