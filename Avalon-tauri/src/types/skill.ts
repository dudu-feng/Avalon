// 技能（Skill）类型
//
// 技能是「可复用的流程知识包」，按需加载，不常驻 system prompt。
// system prompt 只注入 name + description 清单，正文由 agent 用 use_skill 工具按需读取。

/** 技能索引项（SKILL.md 的 frontmatter，不含正文） */
export interface Skill {
  name: string;
  description: string;
}

/** 单条技能 + 正文 + 参考文档列表（get_skill 返回，供详情/编辑模态框） */
export interface SkillDetail {
  name: string;
  description: string;
  content: string;
  /** references/ 下的相对路径列表（如 "foo.md"、"sub/bar.md"），只读展示 */
  references: string[];
}
