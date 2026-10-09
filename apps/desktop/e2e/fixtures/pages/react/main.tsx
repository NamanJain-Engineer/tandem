// The React fixture page (scripts/serve-fixtures.ts `/react`, E2E-M9-4):
// a list of the same components over and over, so a picked element needs a
// real selector to be named, and named components for the picker to find.
import { createRoot } from "react-dom/client";

const TODOS = ["Write the tests", "Make them pass", "Ship it"];

function SaveButton({ label }: { label: string }) {
  return (
    <button type="button" className="save">
      {label}
    </button>
  );
}

function TodoItem({ text }: { text: string }) {
  return (
    <li className="todo">
      <span>{text}</span> <SaveButton label="Save" />
    </li>
  );
}

function App() {
  return (
    <main>
      <h1>React fixture</h1>
      <ul>
        {TODOS.map((text) => (
          <TodoItem key={text} text={text} />
        ))}
      </ul>
    </main>
  );
}

const root = document.getElementById("root");
if (root) createRoot(root).render(<App />);
