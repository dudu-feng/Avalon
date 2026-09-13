// 灵魂注册表条目类型

/** 条目保护级别：system 系统注入（只读且不展示）/ seed 初始化推荐（可编辑不可删）/ user 自由（可增删改） */
export type EntryKind = 'system' | 'seed' | 'user';

/** 条目类别：soul 灵魂 / profile 用户画像 */
export type EntryCategory = 'soul' | 'profile';

/** 条目索引（registry.json 反序列化，不含正文） */
export interface SoulEntry {
  id: string;
  kind: EntryKind;
  category: EntryCategory;
  title: string;
  /** 正文文件路径（相对 prompt 目录），正文存独立 md */
  content_file: string;
  order: number;
  enabled: boolean;
}

/** 单条条目 + 正文（get_soul_entry 返回，供详情/编辑模态框） */
export interface SoulEntryDetail extends SoulEntry {
  content: string;
}
