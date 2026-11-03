<template>
    <div class="invalid-style">
        <span>Aspas duplas e trailing comma</span>
    </div>
</template>

<script setup lang="ts">
    const badString = "aspas duplas no script";
    const badObject = {
        first: 1,
        second: 2,
    };
</script>

<style lang="scss" scoped>
    .invalid-style {
        display: block;
    }
</style>
