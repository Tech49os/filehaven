import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './styles/tokens.css'
import './styles/glass.css'

createApp(App).use(createPinia()).mount('#app')
