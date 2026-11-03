<template>
    <div
        class="multiline-box"
        id="test-multiline">
        <span>Tag multiline inválida</span>
    </div>
</template>

<script setup lang="ts">
    const msg = 'Inválido por tag de abertura em múltiplas linhas';
</script>

<style lang="scss" scoped>
    .multiline-box {
        display: block;
    }
</style>
