<script lang="ts">
  import type { Editor } from '@tiptap/core';

  let { editor }: { editor: Editor | null } = $props();

  type ToolbarAction = {
    label: string;
    icon: string;
    action: () => void;
    isActive: () => boolean;
  };

  const actions: ToolbarAction[] = $derived(
    editor
      ? [
          {
            label: 'Bold',
            icon: 'B',
            action: () => editor!.chain().focus().toggleBold().run(),
            isActive: () => editor!.isActive('bold'),
          },
          {
            label: 'Italic',
            icon: 'I',
            action: () => editor!.chain().focus().toggleItalic().run(),
            isActive: () => editor!.isActive('italic'),
          },
          {
            label: 'Strike',
            icon: 'S',
            action: () => editor!.chain().focus().toggleStrike().run(),
            isActive: () => editor!.isActive('strike'),
          },
          {
            label: 'Code',
            icon: '<>',
            action: () => editor!.chain().focus().toggleCode().run(),
            isActive: () => editor!.isActive('code'),
          },
          {
            label: 'Heading 1',
            icon: 'H1',
            action: () => editor!.chain().focus().toggleHeading({ level: 1 }).run(),
            isActive: () => editor!.isActive('heading', { level: 1 }),
          },
          {
            label: 'Heading 2',
            icon: 'H2',
            action: () => editor!.chain().focus().toggleHeading({ level: 2 }).run(),
            isActive: () => editor!.isActive('heading', { level: 2 }),
          },
          {
            label: 'Heading 3',
            icon: 'H3',
            action: () => editor!.chain().focus().toggleHeading({ level: 3 }).run(),
            isActive: () => editor!.isActive('heading', { level: 3 }),
          },
          {
            label: 'Bullet List',
            icon: '\u2022',
            action: () => editor!.chain().focus().toggleBulletList().run(),
            isActive: () => editor!.isActive('bulletList'),
          },
          {
            label: 'Ordered List',
            icon: '1.',
            action: () => editor!.chain().focus().toggleOrderedList().run(),
            isActive: () => editor!.isActive('orderedList'),
          },
          {
            label: 'Task List',
            icon: '\u2611',
            action: () => editor!.chain().focus().toggleTaskList().run(),
            isActive: () => editor!.isActive('taskList'),
          },
          {
            label: 'Blockquote',
            icon: '\u201C',
            action: () => editor!.chain().focus().toggleBlockquote().run(),
            isActive: () => editor!.isActive('blockquote'),
          },
          {
            label: 'Code Block',
            icon: '{ }',
            action: () => editor!.chain().focus().toggleCodeBlock().run(),
            isActive: () => editor!.isActive('codeBlock'),
          },
          {
            label: 'Horizontal Rule',
            icon: '\u2014',
            action: () => editor!.chain().focus().setHorizontalRule().run(),
            isActive: () => false,
          },
        ]
      : [],
  );
</script>

<div class="toolbar">
  {#each actions as btn (btn.label)}
    <button
      class="toolbar-btn"
      class:active={btn.isActive()}
      onclick={btn.action}
      title={btn.label}
    >
      {btn.icon}
    </button>
  {/each}
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    height: var(--toolbar-height);
    padding: 0 24px;
    gap: 2px;
    border-bottom: var(--border);
    background: var(--white);
  }

  .toolbar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 26px;
    padding: 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--black);
    background: var(--white);
  }

  .toolbar-btn:hover {
    color: var(--accent);
  }

  .toolbar-btn.active {
    background: var(--accent);
    color: var(--white);
  }
</style>
