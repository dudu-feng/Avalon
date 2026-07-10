import os
import numpy as np
from typing import List, Optional
from sentence_transformers import SentenceTransformer


class EmbeddingService:
    _instance: Optional["EmbeddingService"] = None

    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            # 延迟初始化字段，实例创建时先不加载模型
            cls._instance._model: Optional[SentenceTransformer] = None
            cls._instance._model_root: str = ""
            cls._instance._model_name: str = ""
            cls._instance._device: str = ""
        return cls._instance

    def _load_env(self):
        """加载环境变量，只执行一次"""
        try:
            from dotenv import load_dotenv
            load_dotenv()
        except ImportError:
            pass

        self._model_root = os.getenv("model_cache_dir", "")
        self._model_name = os.getenv("local_embedding_model", "")
        self._device = os.getenv("embedding_device", "cpu")

        if not self._model_root or not self._model_name:
            raise RuntimeError("环境变量 model_cache_dir 或 local_embedding_model 不能为空")

        model_path = os.path.join(self._model_root, self._model_name)
        if not os.path.exists(model_path):
            raise FileNotFoundError(f"模型文件夹不存在：{model_path}")
        return model_path

    def _init_model(self):
        """懒加载：第一次调用embedding方法才加载模型"""
        if self._model is not None:
            return

        model_path = self._load_env()
        try:
            self._model = SentenceTransformer(model_path, device=self._device)
            print(f"Embedding模型加载成功，设备：{self._model.device.type}")
        except Exception as e:
            raise RuntimeError(f"模型加载失败：{str(e)}") from e

    def doc_embedding(self, doc_text: str) -> np.ndarray:
        """文档向量，不带指令"""
        self._init_model()
        doc_text = doc_text.strip()
        if not doc_text:
            raise ValueError("文档文本不能为空")
        return self._model.encode(doc_text, normalize_embeddings=True)

    def query_embedding(self, query_text: str) -> np.ndarray:
        """检索query向量，拼接指令"""
        self._init_model()
        instruction = "为这个句子生成表示以用于检索相关文章："
        query_text = query_text.strip()
        if not query_text:
            raise ValueError("查询文本不能为空")
        return self._model.encode(instruction + query_text, normalize_embeddings=True)

    def batch_doc_embedding(self, text_list: List[str]) -> np.ndarray:
        """批量文档向量化，自动过滤空字符串"""
        self._init_model()
        valid_texts = [t.strip() for t in text_list if t.strip()]
        return self._model.encode(valid_texts, normalize_embeddings=True)

    @property
    def model(self) -> Optional[SentenceTransformer]:
        """只读属性，必要时外部可以获取model对象"""
        self._init_model()
        return self._model


# 全局唯一实例，外部统一导入这个实例即可
embedding_service = EmbeddingService()