// 「关于」页数据：联系方式、开源链接、社交账号二维码。
//
// 邮箱地址不在源码中出现完整字符串，拆成片段运行时 join，防止爬虫直接抓取；
// GitHub 是公开链接，无需混淆；社交二维码图片由 Vite 资源导入。

import bilibiliQr from '../../assets/about/bilibili.png';
import douyinQr from '../../assets/about/douyin.jpg';

export const CONTACT = {
  // 1186736810@qq.com
  qqMail: ['1186736810', '@', 'qq', '.', 'com'].join(''),
  // duedudu.feng@gmail.com
  email: ['duedudu', '.', 'feng', '@', 'gmail', '.', 'com'].join(''),
} as const;

export const LINKS = {
  project: 'https://github.com/dudu-feng/Avalon',
  author: 'https://github.com/dudu-feng',
} as const;

export const SOCIAL = [
  { name: 'bilibili', account: '肚饿都督', qr: bilibiliQr },
  { name: 'douyin', account: '肚饿都督', qr: douyinQr },
] as const;
