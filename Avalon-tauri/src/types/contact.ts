/** 渠道通讯录条目：只有 id / name / profile 三个字段 */
export interface Contact {
  /** 规范 id，带渠道前缀，如 feishu:ou_xxx / qq:xxx */
  id: string;
  /** 显示名 */
  name: string;
  /** 用户画像，自由文本 */
  profile: string;
}

/** 渠道主人条目：只有 id 与昵称，没有画像（画像归灵魂模块） */
export interface OwnerContact {
  /** 规范 id，带渠道前缀，如 feishu:ou_xxx / qq:xxx */
  id: string;
  /** 昵称；飞书事件可能拿不到，允许为空 */
  name: string;
}

/** 通讯录列表结果：owners 按渠道名（feishu / qq）索引 */
export interface ContactList {
  total: number;
  users: Contact[];
  owners: Record<string, OwnerContact>;
}
