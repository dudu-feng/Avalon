# zvec_store.py
import os
import zvec
from zvec.model.param.query import Fts
from config.env_config import env_config
from loop.embedding_service import embedding_service


class ZvecStore:
    # 单例实例
    _instance = None

    def __new__(cls, *args, **kwargs):
        """实现单例"""
        if cls._instance is None:
            cls._instance = super().__new__(cls)
            cls._instance._initialized = False
        return cls._instance

    def __init__(self):
        if self._initialized:
            return

        # 1. 定义集合Schema
        self._session_memory_schema = zvec.CollectionSchema(
            name="Avalon_session_memory_index",
            fields=[
                zvec.FieldSchema(
                    name="description",
                    data_type=zvec.DataType.STRING,
                    nullable=False,
                    index_param=zvec.FtsIndexParam(
                        tokenizer_name="jieba",
                    ),
                )
            ],
            vectors=[
                zvec.VectorSchema(
                    name="summary_vector",
                    data_type=zvec.DataType.VECTOR_FP32,
                    dimension=512,
                    index_param=zvec.HnswIndexParam(metric_type=zvec.MetricType.COSINE),
                ),
            ],
        )

        # 2. 获取数据库路径（使用配置类提供的派生路径）
        self._zvec_collection_path = env_config.zvec_db_path

        # 3. 创建并打开集合，私有成员外部禁止访问
        self._collection = zvec.open(
            path=self._zvec_collection_path,
            option=zvec.CollectionOption(read_only=False, enable_mmap=True),
        )
        self._initialized = True

    @property
    def collection(self):
        """只读暴露collection，不允许外部修改"""
        return self._collection

    def insert_session_memory(self, doc_id: str, text: str):
        """插入会话记忆"""
        vector = embedding_service.doc_embedding(text)
        doc = zvec.Doc(
            id=doc_id,
            vectors={
                "summary_vector": vector,
            },
            fields={
                "description": text,
            },
        )
        result = self._collection.insert(doc)
        return result

    def upsert_session_memory(self, doc_id: str, text: str):
        """更新或插入会话记忆"""
        vector = embedding_service.doc_embedding(text)
        doc = zvec.Doc(
            id=doc_id,
            vectors={
                "summary_vector": vector,
            },
            fields={
                "description": text,
            },
        )
        result = self._collection.upsert(doc)
        return result

    def delete_session_memory(self, doc_id: str):
        """删除会话记忆"""
        result = self._collection.delete(ids = doc_id)
        return result

    def batch_delete_session_memory(self, doc_ids: list[str]):
        """批量删除会话记忆"""
        result = self._collection.delete(ids = doc_ids)
        return result

    def vectorQuery_session_memory(self, queryContent: str, topk: int = 5):
        """查询会话记忆向量相似度"""
        queryVector = embedding_service.query_embedding(queryContent)
        result = self._collection.query(
            queries=zvec.Query(
                field_name = "summary_vector",
                vector = queryVector,
            ),
            topk = topk,
            include_vector=False,
        )
        return result

    def scalarQuery_session_memory(self, queryContent: str, topk: int = 5):
        """查询会话记忆文本相似度"""
        result = self._collection.query(
            queries=zvec.Query(
                field_name = "description",
                fts=Fts( match_string= queryContent ),
            ),
            topk = topk,
        )
        return result


# 全局唯一实例，其它模块只导入这个实例即可
zvec_store = ZvecStore()
