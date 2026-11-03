<template>
    <div class="valid-box">
        <span id="test-title">{{ message }}</span>
        <button type="button" @click="increment">{{ count }}</button>
    </div>
</template>

<script setup lang="ts">
    import { ref } from 'vue';

    const count = ref<number>(0);
    const message = 'Componente estritamente válido';

    function increment(): void {
        count.value += 1;
    }
</script>

<style lang="scss" scoped>
    .valid-box {
        display: flex;
        color: var(--max-primary-500);
    }
</style>
