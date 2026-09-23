import { SimProvider } from './context/SimContext'
import Dashboard from './components/Dashboard'

export default function App() {
  return (
    <SimProvider>
      <Dashboard />
    </SimProvider>
  )
}
