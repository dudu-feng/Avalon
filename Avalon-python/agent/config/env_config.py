"""
环境变量配置中心 —— 唯一的环境变量加载入口

所有模块通过 EnvConfig 单例获取配置项，不再各自调用 load_dotenv()。

使用方式：
    from config.env_config import env_config

    api_key = env_config.default_api_key
    model   = env_config.default_model
    ...
"""

import os
from typing import Optional


class EnvConfig:
    """环境变量配置类（单例）

    首次实例化时加载 .env 文件，后续返回缓存实例。
    通过属性（property）暴露所有配置项，调用方不需要关心
    变量名、默认值、以及 .env 是否存在等细节。
    """

    _instance: Optional["EnvConfig"] = None

    def __new__(cls) -> "EnvConfig":
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._initialized = False
        return cls._instance

    def __init__(self) -> None:
        if self._initialized:
            return
        self._load_dotenv()
        self._initialized = True

    # ---- 内部方法 ----

    def _load_dotenv(self) -> None:
        """加载 .env 文件（只执行一次）

        从当前文件所在目录向上查找 agent 目录下的 .env 文件，
        这样可以避免依赖调用方的工作目录。
        """
        # config/env_config.py → agent/config/ → agent/
        _config_dir = os.path.dirname(os.path.abspath(__file__))
        _agent_dir = os.path.dirname(_config_dir)
        _dotenv_path = os.path.join(_agent_dir, ".env")
        print(_dotenv_path)

        try:
            from dotenv import load_dotenv
            load_dotenv(_dotenv_path)
        except ImportError:
            print("⚠️ 未安装 python-dotenv，将使用系统环境变量或默认值")

    @staticmethod
    def _get(key: str, default: str = "") -> str:
        """读取环境变量，若不存在则返回默认值"""
        return os.getenv(key, default)

    # ============================================================
    #  LLM 配置
    # ============================================================

    @property
    def default_api_key(self) -> str:
        return self._get("default_api_key")

    @property
    def default_model(self) -> str:
        return self._get("default_model")

    @property
    def default_model_base_url(self) -> str:
        return self._get("default_model_base_url")

    # ============================================================
    #  路径配置（全部使用绝对路径）
    # ============================================================

    @property
    def prompt_file_path(self) -> str:
        return self._get("prompt_file_path")

    @property
    def memory_path(self) -> str:
        return self._get("memory_path")

    @property
    def session_path(self) -> str:
        return self._get("session_path")

    @property
    def session_index_path(self) -> str:
        return self._get("session_index_path")

    # ============================================================
    #  向量数据库
    # ============================================================

    @property
    def vector_db_path(self) -> str:
        return self._get("vector_db_path")

    @property
    def model_cache_dir(self) -> str:
        return self._get("model_cache_dir")

    # ============================================================
    #  Embedding 模型配置
    # ============================================================

    @property
    def embedding_mode(self) -> str:
        """'local' 使用本地模型，'api' 使用 API 模型"""
        return self._get("embedding_mode", "local")

    @property
    def local_embedding_model(self) -> str:
        return self._get("local_embedding_model")

    @property
    def embedding_device(self) -> str:
        return self._get("embedding_device", "cpu")

    @property
    def api_embedding_key(self) -> str:
        return self._get("api_embedding_key")

    @property
    def api_embedding_model(self) -> str:
        return self._get("api_embedding_model")

    @property
    def api_embedding_base_url(self) -> str:
        return self._get("api_embedding_base_url")

    # ============================================================
    #  派生属性（组合多个变量计算得到）
    # ============================================================

    @property
    def local_embedding_model_path(self) -> str:
        """本地 embedding 模型完整路径 = model_cache_dir + local_embedding_model"""
        return os.path.join(self.model_cache_dir, self.local_embedding_model)

    @property
    def chroma_db_path(self) -> str:
        """ChromaDB 持久化路径"""
        return os.path.join(self.vector_db_path, "chroma")

    @property
    def zvec_db_path(self) -> str:
        """ZVec 持久化路径"""
        return os.path.join(self.vector_db_path, "zvec")


# 全局唯一实例，其它模块只导入这个实例即可
env_config = EnvConfig()
