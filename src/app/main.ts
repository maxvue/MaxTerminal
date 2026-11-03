import 'virtual:uno.css';
import '@/styles/main.scss';
import { createApp } from 'vue';
import { applyThemeBootstrapSync } from '@/theme/bootstrap';
import App from './App.vue';
import { installGlobalErrorHandlers } from './logger';
import { registerAppPlugins } from './plugins';

applyThemeBootstrapSync();

const app = createApp(App);
installGlobalErrorHandlers(app);
registerAppPlugins(app);
app.mount('#app');
