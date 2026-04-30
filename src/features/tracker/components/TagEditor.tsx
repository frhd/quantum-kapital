import { useState } from "react"
import { Button } from "../../../shared/components/ui/button"
import { Input } from "../../../shared/components/ui/input"
import { X } from "lucide-react"
import { BUILT_IN_TAGS, type StrategyTag } from "../types"

interface TagEditorProps {
  tags: StrategyTag[]
  onSave: (next: StrategyTag[]) => Promise<unknown> | void
  onCancel: () => void
}

export function TagEditor({ tags, onSave, onCancel }: TagEditorProps) {
  const [draftTags, setDraftTags] = useState<StrategyTag[]>(() => [...tags])
  const [draftCustom, setDraftCustom] = useState("")
  const [saving, setSaving] = useState(false)

  const toggleDraftTag = (tag: StrategyTag) => {
    setDraftTags((prev) => (prev.includes(tag) ? prev.filter((t) => t !== tag) : [...prev, tag]))
  }

  const addCustomDraft = () => {
    const t = draftCustom.trim()
    if (!t) return
    if (!draftTags.includes(t)) {
      setDraftTags([...draftTags, t])
    }
    setDraftCustom("")
  }

  const handleSave = async () => {
    setSaving(true)
    try {
      await onSave(draftTags)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-1">
        {BUILT_IN_TAGS.map((opt) => {
          const active = draftTags.includes(opt.value)
          return (
            <button
              key={opt.value}
              type="button"
              onClick={() => toggleDraftTag(opt.value)}
              className={
                "rounded-full border px-2 py-0.5 text-xs transition-colors " +
                (active
                  ? "border-blue-400 bg-blue-500/20 text-blue-100"
                  : "border-slate-600 bg-slate-800 text-slate-300 hover:bg-slate-700")
              }
            >
              {opt.label}
            </button>
          )
        })}
        {draftTags
          .filter((tag) => !BUILT_IN_TAGS.some((b) => b.value === tag))
          .map((tag) => (
            <button
              key={tag}
              type="button"
              onClick={() => toggleDraftTag(tag)}
              className="flex items-center gap-1 rounded-full border border-blue-400 bg-blue-500/20 px-2 py-0.5 text-xs text-blue-100"
            >
              {tag}
              <X className="h-3 w-3" />
            </button>
          ))}
      </div>
      <div className="flex gap-1">
        <Input
          value={draftCustom}
          onChange={(e) => setDraftCustom(e.target.value)}
          placeholder="Custom tag…"
          className="h-7 bg-slate-900 text-xs"
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault()
              addCustomDraft()
            }
          }}
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-7 px-2 text-xs"
          onClick={addCustomDraft}
          disabled={!draftCustom.trim()}
        >
          Add
        </Button>
      </div>
      <div className="flex justify-end gap-2">
        <Button variant="outline" size="sm" onClick={onCancel} disabled={saving}>
          Cancel
        </Button>
        <Button size="sm" onClick={handleSave} disabled={saving}>
          {saving ? "Saving…" : "Save"}
        </Button>
      </div>
    </div>
  )
}
