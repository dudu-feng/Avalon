// 技能命令的接口封装
//
// 组件不直接 invoke，而是通过这里的语义化函数调用。
// 用户侧只读 + 删除；技能由智能体在对话中沉淀或按链接拉取，不开放人工自由创建/编辑。

import { invoke } from '@tauri-apps/api/core';
import type { Skill, SkillDetail } from '../types/skill';

/** 列出全部技能（name + description，按 name 排序） */
export async function listSkills(): Promise<Skill[]> {
  return invoke<Skill[]>('list_skills');
}

/** 读取单个技能正文（查看详情模态框用） */
export async function getSkill(name: string): Promise<SkillDetail> {
  return invoke<SkillDetail>('get_skill', { name });
}

/** 删除技能（删除其整个目录） */
export async function deleteSkill(name: string): Promise<void> {
  return invoke<void>('delete_skill', { name });
}
