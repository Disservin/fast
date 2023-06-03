<script setup lang="ts">
// Utilities
import { ref } from "vue";

const props = defineProps<{
	fen: string;
}>();

const emit = defineEmits(["update-position"]);

const currentFen = ref(props.fen);

const updatePosition = () => {
	emit("update-position", currentFen.value);
};
</script>

<template>
  <div>
    <div class="fen-input">
      <v-text-field
        id="fen-input"
        v-model="currentFen"
        hide-details="auto"
        @input="updatePosition"
      />
    </div>
  </div>
</template>

<style scoped>
@import "@/assets/styles/variables.css";

.fen-input {
	margin-top: 5px;
	margin-bottom: 5px;
}
</style>
