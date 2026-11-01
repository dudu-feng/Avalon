/** 渠道通讯录条目：只有 id / name / profile 三个字段 */
export interface Contact {
  /** 规范 id，带渠道前缀，如 feishu:ou_xxx / qq:xxx */
  id: string;
  /** 显示名 */
  name: string;
  /** 用户画像，自由文本 */
  profile: string;
}

/** 通讯录列表结果：total 是全量人数，users 是本次匹配结果 */
export interface ContactList {
  total: number;
  users: Contact[];
}
