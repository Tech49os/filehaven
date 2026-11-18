/**
 * 扫描时该不该收这份文件。
 *
 * 真实数据暴露的问题：扫描 455 份里有 143 份没能归类，其中一大半根本不是文档——
 * `.lnk` 快捷方式、`.exe`、`desktop.ini`、`.yaml` 配置、哈希名图片。
 * 它们读不出正文，注定归不了类，留在列表里只会：
 *   1. 把「未归类」这个数字撑得很难看，让用户以为分类能力不行
 *   2. 在确认页和微调页堆成噪音，真正需要处理的文件被淹掉
 *
 * 所以按「能不能读出正文/能不能看内容」划一条线：判断不了事项的文件，
 * 一开始就别收进来。跳过的数量会在扫描进度里如实显示，不偷偷丢。
 */
import { extOf } from './extractor'

/**
 * 明确不是「要归档的文档」的扩展名。
 * 只放语言级/系统级的常识词，不涉及任何行业——换谁的电脑这些都一样。
 */
const NON_DOC_EXT = new Set([
  // 快捷方式与可执行
  'lnk', 'exe', 'msi', 'dll', 'sys', 'com', 'scr', 'bat', 'cmd', 'ps1', 'reg', 'vbs',
  // 系统与临时
  'ini', 'tmp', 'temp', 'thumbcache', 'db', 'dat', 'log1', 'bak1',
  // 压缩包：读不出正文，归类只能靠猜
  'zip', 'rar', '7z', 'gz', 'tar', 'iso', 'cab',
  // 二进制 / 编译产物
  'pyc', 'class', 'jar', 'so', 'dylib', 'obj', 'o', 'bin', 'apk', 'dmg',
])

/** 系统自己生成的固定文件名，带不带扩展名都一样处理 */
const NON_DOC_NAMES = new Set(['thumbs.db', 'ehthumbs.db', 'desktop.ini', 'icon\r'])

export function isNonDocument(name: string): boolean {
  const lower = name.toLowerCase()
  if (NON_DOC_NAMES.has(lower)) return true
  // 无扩展名的文件不武断排除：可能是不带后缀的正文文件
  const ext = extOf(name).toLowerCase()
  if (!ext) return false
  return NON_DOC_EXT.has(ext)
}
