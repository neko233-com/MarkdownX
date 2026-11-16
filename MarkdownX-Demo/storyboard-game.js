const slate = document.getElementById('slate');
const brush = slate.getContext('2d');
const caption = document.createElement('p');
slate.insertAdjacentElement('afterend', caption);
let takes = 0;
function drawSlate() {
  brush.clearRect(0, 0, slate.width, slate.height);
  brush.fillStyle = '#f7e8bd';
  brush.font = 'bold 24px sans-serif';
  brush.fillText('ACTION', 86, 74);
  caption.textContent = '已拍摄镜次：' + takes;
}
slate.addEventListener('click', () => { takes++; drawSlate(); });
drawSlate();
