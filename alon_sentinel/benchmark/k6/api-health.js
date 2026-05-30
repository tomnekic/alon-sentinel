import http from 'k6/http';

export const options = {
  vus: 50,
  duration: '60s',
};

export default function () {
  http.get('http://host.docker.internal:3000/health');
}
