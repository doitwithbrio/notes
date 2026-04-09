<script lang="ts">
  let {
    onClickRegion,
  }: {
    onClickRegion?: (targetId: string) => void;
  } = $props();
</script>

<button data-testid="mock-change-minibar" type="button" onclick={() => onClickRegion?.('changed-1-0')}>
  jump to diff
</button>
