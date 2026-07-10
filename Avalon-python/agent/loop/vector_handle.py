import chromadb
import os
import uuid
from typing import List, Dict, Any, Optional
from chromadb.config import Settings
from chromadb.utils import embedding_functions

from config.env_config import env_config


class LocalEmbeddingFunction(embedding_functions.EmbeddingFunction):
    """本地模型Embedding函数类"""

    def __init__(self, model_name: str = "BAAI/bge-small-zh-v1.5", device: str = "cpu"):
        """
        初始化本地embedding模型

        Args:
            model_name: 模型名称，默认为 BAAI/bge-small-zh-v1.5
            device: 运行设备，'cpu' 或 'cuda'
        """
        self.model_name = model_name
        self.device = device
        self.model = None
        self._load_model()

    def _load_model(self):
        """加载本地模型，优先从本地目录加载"""
        try:
            from sentence_transformers import SentenceTransformer

            # 设置国内镜像（以防需要下载）
            os.environ.setdefault("HF_ENDPOINT", "https://hf-mirror.com")

            # 设置模型缓存目录
            cache_dir = env_config.model_cache_dir
            os.makedirs(cache_dir, exist_ok=True)

            # 检查本地是否已存在模型
            model_local_path = os.path.join(
                cache_dir,
                "bge-small-zh-v1.5",
                "models--BAAI--bge-small-zh-v1.5",
                "snapshots",
                "7999e1d3359715c523056ef9478215996d62a620"
            )

            if os.path.exists(model_local_path) and os.path.isdir(model_local_path):
                print(f"🔄 从本地目录加载模型: {model_local_path}")
                load_path = model_local_path
            else:
                print(f"🔄 正在加载模型: {self.model_name} (缓存目录: {cache_dir})")
                load_path = self.model_name

            # 加载模型
            self.model = SentenceTransformer(
                load_path,
                device=self.device,
                cache_folder=cache_dir
            )
            print(f"✅ 模型加载成功: {self.model_name}")
        except ImportError:
            raise ImportError(
                "请安装 sentence-transformers 库: pip install sentence-transformers"
            )
        except Exception as e:
            raise RuntimeError(f"本地模型加载失败: {e}")

    def __call__(self, texts: List[str]) -> List[List[float]]:
        """
        将文本转换为向量

        Args:
            texts: 文本列表

        Returns:
            List[List[float]]: 向量列表
        """
        if self.model is None:
            self._load_model()

        try:
            # 生成embeddings
            embeddings = self.model.encode(
                texts,
                convert_to_numpy=True,
                normalize_embeddings=True,
                show_progress_bar=False
            )

            # 转换为列表格式
            return embeddings.tolist()
        except Exception as e:
            raise RuntimeError(f"文本向量化失败: {e}")

class VectorHandle:
    """向量数据库处理类，负责向量转化、存储和检索"""

    def __init__(
            self,
            collection_name: str = "avalon_session_memory"
        ):
        """
        初始化向量数据库

        Args:
            collection_name: 集合名称，默认为 'avalon_memory'
        """
        self.collection_name = collection_name
        self.client = None
        self.collection = None
        self.embedding_function = None
        self._init_client()

    def _init_client(self):
        """初始化ChromaDB客户端和集合"""
        vector_db_path = env_config.chroma_db_path
        os.makedirs(vector_db_path, exist_ok=True)

        self.client = chromadb.PersistentClient(path=vector_db_path)
        self._init_local_embedding()

        try:
            self.collection = self.client.get_collection(
                name=self.collection_name,
                embedding_function=self.embedding_function
            )
        except:
            self.collection = self.client.create_collection(
                name=self.collection_name,
                embedding_function=self.embedding_function,
                metadata={
                    "description": "Avalon智能体记忆向量数据库"
                }
            )

    def _init_local_embedding(self):
        """初始化本地embedding模型"""
        try:
            print(f"🔄 使用本地embedding模式: BAAI/bge-small-zh-v1.5")
            self.embedding_function = LocalEmbeddingFunction(
                model_name="BAAI/bge-small-zh-v1.5",
                device=env_config.embedding_device
            )
            print(f"✅ 本地embedding模型初始化成功")
        except Exception as e:
            raise RuntimeError(f"本地embedding模型初始化失败: {e}")

    def add_documents(
            self,
            documents: List[str],
            metadatas: Optional[List[Dict[str, Any]]] = None,
            ids: Optional[List[str]] = None
        ) -> bool:
        """
        添加文档到向量数据库

        Args:
            documents: 文档文本列表
            metadatas: 文档元数据列表
            ids: 文档ID列表

        Returns:
            bool: 是否添加成功
        """
        try:
            if not documents:
                return False

            # 如果没有提供ids，自动生成
            if ids is None:
                ids = [str(uuid.uuid4()) for _ in documents]

            # 如果没有提供metadatas，创建包含默认字段的元数据（ChromaDB要求非空）
            if metadatas is None:
                metadatas = [{"_default": ""} for _ in documents]

            # 添加到集合
            self.collection.add(
                documents=documents,
                metadatas=metadatas,
                ids=ids
            )
            return True
        except Exception as e:
            print(f"❌ 添加文档失败: {e}")
            return False

    def search(
            self,
            query: str,
            n_results: int = 5
        ) -> List[Dict[str, Any]]:
        """
        在向量数据库中搜索相关文档

        Args:
            query: 查询文本
            n_results: 返回结果数量

        Returns:
            List[Dict[str, Any]]: 搜索结果列表
        """
        try:
            results = self.collection.query(
                query_texts=[query],
                n_results=n_results
            )

            # 格式化结果
            formatted_results = []
            if results['documents'] and results['documents'][0]:
                for i, doc in enumerate(results['documents'][0]):
                    formatted_results.append({
                        "id": results['ids'][0][i],
                        "document": doc,
                        "metadata": results['metadatas'][0][i] if results['metadatas'] else {},
                        "distance": results['distances'][0][i] if results['distances'] else 0
                    })

            return formatted_results
        except Exception as e:
            print(f"❌ 搜索失败: {e}")
            return []

    def get_collection_info(self) -> Dict[str, Any]:
        """
        获取集合信息

        Returns:
            Dict[str, Any]: 集合信息
        """
        try:
            count = self.collection.count()
            return {
                "name": self.collection_name,
                "count": count,
                "metadata": self.collection.metadata
            }
        except Exception as e:
            print(f"❌ 获取集合信息失败: {e}")
            return {}

# 创建全局实例
_vector_handle = None

def get_vector_handle(
        collection_name: str = "avalon_memory"
    ) -> VectorHandle:
    """
    获取向量处理实例（单例模式）

    Args:
        collection_name: 集合名称

    Returns:
        VectorHandle: 向量处理实例
    """
    global _vector_handle

    if _vector_handle is None or _vector_handle.collection_name != collection_name:
        _vector_handle = VectorHandle(collection_name)

    return _vector_handle
