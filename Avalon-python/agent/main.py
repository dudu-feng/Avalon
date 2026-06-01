from agent.agent import chat_with_agent

if __name__ == "__main__":
    print("🤖 Avalon Agent 已启动！输入 exit 退出。\n")

    while True:
        user_input = input("你: ")
        if user_input.lower() in ["exit", "quit", "退出"]:
            print("再见！")
            break

        if not user_input.strip():
            continue

        try:
            response = chat_with_agent(user_input)
            print(f"\nAgent: {response}\n")
        except Exception as e:
            print(f"错误: {e}\n")
