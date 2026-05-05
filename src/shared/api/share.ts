/**
 * Wrapper for the `save_share_image_png` Tauri command. Pops the native
 * save dialog and writes the PNG bytes to the chosen path. Returns the
 * saved path, or `null` if the user cancelled. The dialog and write
 * both live on the Rust side because Tauri webviews block `<a download>`
 * and the JS clipboard API rejects image MIME types.
 */

import { invoke } from "@tauri-apps/api/core"

export const shareApi = {
  saveShareImagePng: async (date: string, bytes: Uint8Array): Promise<string | null> => {
    return invoke<string | null>("save_share_image_png", { date, bytes })
  },
}
