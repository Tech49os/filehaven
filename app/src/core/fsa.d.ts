export {}

declare global {
  interface Window {
    showDirectoryPicker?: (opts?: {
      mode?: 'read' | 'readwrite'
      startIn?: string
    }) => Promise<FileSystemDirectoryHandle>
  }

  /** Chrome 已实现，部分 TS lib 未声明 */
  interface FileSystemFileHandle {
    remove?: () => Promise<void>
  }
}
