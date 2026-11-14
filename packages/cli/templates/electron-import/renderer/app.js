const note = document.querySelector('#note');
const status = document.querySelector('#status');

document.getElementById('save').addEventListener('click', async () => {
  status.textContent = await window.api.saveNote(note.value);
});

document.getElementById('load').addEventListener('click', async () => {
  note.value = await window.api.loadNote();
  status.textContent = 'Loaded';
});
